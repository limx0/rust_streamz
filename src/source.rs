use std::cell::RefCell;
use std::mem;
use std::ops::Deref;
use std::rc::Rc;
use std::time::Duration;

type Callback<T> = Rc<dyn Fn(&T)>;

pub struct Source<T> {
    callbacks: Rc<RefCell<Vec<Callback<T>>>>,
}

impl<T> Default for Source<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Source<T> {
    pub fn new() -> Self {
        Self {
            callbacks: Rc::new(RefCell::new(Vec::new())),
        }
    }

    pub fn emit(&self, item: T) {
        let callbacks = self.callbacks.borrow();
        for callback in callbacks.iter() {
            callback(&item);
        }
    }

    pub fn to_stream(&self) -> Stream<T> {
        Stream {
            callbacks: self.callbacks.clone(),
        }
    }
}

pub struct Stream<T> {
    callbacks: Rc<RefCell<Vec<Callback<T>>>>,
}

impl<T> Stream<T> {
    pub fn map<U, F>(&self, f: F) -> Stream<U>
    where
        U: 'static,
        F: Fn(&T) -> U + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<U>>::new()));
        let downstream_clone = downstream.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            let mapped = f(item);
            for callback in downstream_clone.borrow().iter() {
                callback(&mapped);
            }
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn filter<F>(&self, predicate: F) -> Stream<T>
    where
        T: 'static,
        F: Fn(&T) -> bool + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<T>>::new()));
        let downstream_clone = downstream.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            if predicate(item) {
                for callback in downstream_clone.borrow().iter() {
                    callback(item);
                }
            }
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn filter_map<U, F>(&self, f: F) -> Stream<U>
    where
        U: 'static,
        F: Fn(&T) -> Option<U> + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<U>>::new()));
        let downstream_clone = downstream.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            if let Some(mapped) = f(item) {
                for callback in downstream_clone.borrow().iter() {
                    callback(&mapped);
                }
            }
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn timed_buffer(&self, period: Duration) -> TimedBuffer<T>
    where
        T: Clone + 'static,
    {
        let callbacks: Rc<RefCell<Vec<Callback<Vec<T>>>>> = Rc::new(RefCell::new(Vec::new()));
        let stream = Stream {
            callbacks: callbacks.clone(),
        };
        let buffer = Rc::new(RefCell::new(Vec::<T>::new()));
        let buffer_clone = buffer.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            buffer_clone.borrow_mut().push(item.clone());
        }));

        TimedBuffer::new(period, buffer, callbacks, stream)
    }

    pub fn accumulate<State, F>(&self, initial_state: State, f: F) -> Stream<State>
    where
        State: Clone + 'static,
        F: Fn(State, &T) -> State + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<State>>::new()));
        let downstream_clone = downstream.clone();
        let state_cell = Rc::new(RefCell::new(initial_state));
        let state_cell_clone = state_cell.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            let current = state_cell_clone.borrow().clone();
            let next = f(current, item);
            *state_cell_clone.borrow_mut() = next.clone();
            for callback in downstream_clone.borrow().iter() {
                callback(&next);
            }
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn tap<F>(&self, f: F) -> Stream<T>
    where
        T: Clone + 'static,
        F: Fn(&T) + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<T>>::new()));
        let downstream_clone = downstream.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            f(item);
            let cloned = item.clone();
            for callback in downstream_clone.borrow().iter() {
                callback(&cloned);
            }
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn zip<U>(&self, other: &Stream<U>) -> Stream<(T, U)>
    where
        T: Clone + 'static,
        U: Clone + 'static,
    {
        let downstream = Rc::new(RefCell::new(Vec::<Callback<(T, U)>>::new()));
        let downstream_left = downstream.clone();

        let left_state = Rc::new(RefCell::new(None::<T>));
        let right_state = Rc::new(RefCell::new(None::<U>));
        let left_state_left = left_state.clone();
        let right_state_left = right_state.clone();
        let right_state_right = right_state.clone();

        self.callbacks.borrow_mut().push(Rc::new(move |item: &T| {
            {
                *left_state_left.borrow_mut() = Some(item.clone());
            }

            if let (Some(left), Some(right)) = (
                left_state_left.borrow().clone(),
                right_state_left.borrow().clone(),
            ) {
                let pair = (left, right);
                let callbacks = downstream_left.borrow();
                for callback in callbacks.iter() {
                    callback(&pair);
                }
            }
        }));

        other.callbacks.borrow_mut().push(Rc::new(move |item: &U| {
            *right_state_right.borrow_mut() = Some(item.clone());
        }));

        Stream {
            callbacks: downstream,
        }
    }

    pub fn sink<F>(&self, f: F)
    where
        F: Fn(&T) + 'static,
    {
        self.callbacks
            .borrow_mut()
            .push(Rc::new(move |item: &T| f(item)));
    }
}

impl<T> Clone for Stream<T> {
    fn clone(&self) -> Self {
        Stream {
            callbacks: self.callbacks.clone(),
        }
    }
}

pub trait TimedEmitter: 'static {
    fn period(&self) -> Duration;
    fn flush(&self);
}

pub struct TimedBuffer<T> {
    inner: Rc<TimedBufferInner<T>>,
}

struct TimedBufferInner<T> {
    period: Duration,
    buffer: Rc<RefCell<Vec<T>>>,
    callbacks: Rc<RefCell<Vec<Callback<Vec<T>>>>>,
    stream: Stream<Vec<T>>,
}

impl<T> TimedBuffer<T>
where
    T: Clone + 'static,
{
    fn new(
        period: Duration,
        buffer: Rc<RefCell<Vec<T>>>,
        callbacks: Rc<RefCell<Vec<Callback<Vec<T>>>>>,
        stream: Stream<Vec<T>>,
    ) -> Self {
        Self {
            inner: Rc::new(TimedBufferInner {
                period,
                buffer,
                callbacks,
                stream,
            }),
        }
    }

    pub fn stream(&self) -> Stream<Vec<T>> {
        self.inner.stream.clone()
    }

    pub fn period(&self) -> Duration {
        self.inner.period
    }

    pub fn as_timed_emitter(&self) -> Rc<dyn TimedEmitter> {
        self.inner.clone() as Rc<dyn TimedEmitter>
    }
}

impl<T> Clone for TimedBuffer<T>
where
    T: Clone + 'static,
{
    fn clone(&self) -> Self {
        TimedBuffer {
            inner: self.inner.clone(),
        }
    }
}

impl<T> Deref for TimedBuffer<T>
where
    T: Clone + 'static,
{
    type Target = Stream<Vec<T>>;

    fn deref(&self) -> &Self::Target {
        &self.inner.stream
    }
}

impl<T> TimedEmitter for TimedBufferInner<T>
where
    T: Clone + 'static,
{
    fn period(&self) -> Duration {
        self.period
    }

    fn flush(&self) {
        let chunk = {
            let mut buffer = self.buffer.borrow_mut();
            if buffer.is_empty() {
                return;
            }
            mem::take(&mut *buffer)
        };

        let callbacks = self.callbacks.borrow();
        for callback in callbacks.iter() {
            callback(&chunk);
        }
    }
}
