//! Generating UUIDs from timestamps.
//!
//! Timestamps are used in a few UUID versions as a source of decentralized
//! uniqueness (as in versions 1 and 6), and as a way to enable sorting (as
//! in versions 6 and 7). Timestamps aren't encoded the same way by all UUID
//! versions so this module provides a single [`Timestamp`] type that can
//! convert between them.

/// The number of 100 nanosecond ticks between the RFC4122 epoch
/// (`1582-10-15 00:00:00`) and the Unix epoch (`1970-01-01 00:00:00`).
pub const UUID_TICKS_BETWEEN_EPOCHS: u64 = 0x01B2_1DD2_1381_4000;

/// A timestamp that can be encoded into a UUID.
///
/// This type abstracts the specific encoding, so a UUID version 1 or
/// a UUID version 7 can both be supported through the same type, even
/// though they have a different representation of a timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Timestamp {
    pub(crate) seconds: u64,
    pub(crate) nanos: u32,
    #[cfg(any(feature = "v1", feature = "v6"))]
    pub(crate) counter: u16,
}

impl Timestamp {
    /// Get a timestamp representing the current system time.
    ///
    /// This method defers to the standard library's `SystemTime` type.
    #[cfg(feature = "std")]
    pub fn now(context: impl ClockSequence<Output = u16>) -> Self {
        #[cfg(not(any(feature = "v1", feature = "v6")))]
        {
            let _ = context;
        }

        let dur = std::time::SystemTime::UNIX_EPOCH
            .elapsed()
            .expect("Getting elapsed time since UNIX_EPOCH. If this fails, we've somehow violated causality");

        Timestamp {
            seconds: dur.as_secs(),
            nanos: dur.subsec_nanos(),
            #[cfg(any(feature = "v1", feature = "v6"))]
            counter: context.generate_sequence(dur.as_secs(), dur.subsec_nanos()),
        }
    }

    /// Construct a `Timestamp` from an RFC4122 timestamp and counter, as used
    /// in version 1 and version 6 UUIDs.
    pub const fn from_rfc4122(ticks: u64, counter: u16) -> Self {
        #[cfg(not(any(feature = "v1", feature = "v6")))]
        {
            let _ = counter;
        }

        let (seconds, nanos) = Self::rfc4122_to_unix(ticks);

        Timestamp {
            seconds,
            nanos,
            #[cfg(any(feature = "v1", feature = "v6"))]
            counter,
        }
    }

    /// Construct a `Timestamp` from a Unix timestamp.
    pub fn from_unix(context: impl ClockSequence<Output = u16>, seconds: u64, nanos: u32) -> Self {
        #[cfg(not(any(feature = "v1", feature = "v6")))]
        {
            let _ = context;

            Timestamp { seconds, nanos }
        }
        #[cfg(any(feature = "v1", feature = "v6"))]
        {
            let counter = context.generate_sequence(seconds, nanos);

            Timestamp {
                seconds,
                nanos,
                counter,
            }
        }
    }

    /// Get the value of the timestamp as an RFC4122 timestamp and counter,
    /// as used in version 1 and version 6 UUIDs.
    #[cfg(any(feature = "v1", feature = "v6"))]
    pub const fn to_rfc4122(&self) -> (u64, u16) {
        (
            Self::unix_to_rfc4122_ticks(self.seconds, self.nanos),
            self.counter,
        )
    }

    /// Get the value of the timestamp as a Unix timestamp, consisting of the
    /// number of whole and fractional seconds.
    pub const fn to_unix(&self) -> (u64, u32) {
        (self.seconds, self.nanos)
    }

    #[cfg(any(feature = "v1", feature = "v6"))]
    const fn unix_to_rfc4122_ticks(seconds: u64, nanos: u32) -> u64 {
        let ticks = UUID_TICKS_BETWEEN_EPOCHS + seconds * 10_000_000 + nanos as u64 / 100;

        ticks
    }

    const fn rfc4122_to_unix(ticks: u64) -> (u64, u32) {
        (
            (ticks - UUID_TICKS_BETWEEN_EPOCHS) / 10_000_000,
            ((ticks - UUID_TICKS_BETWEEN_EPOCHS) % 10_000_000) as u32 * 100,
        )
    }

    #[deprecated(note = "use `to_unix` instead")]
    /// Get the number of fractional nanoseconds in the Unix timestamp.
    ///
    /// This method is deprecated and probably doesn't do what you're expecting it to.
    /// It doesn't return the timestamp as nanoseconds since the Unix epoch, it returns
    /// the fractional seconds of the timestamp.
    pub const fn to_unix_nanos(&self) -> u32 {
        // NOTE: This method never did what it said on the tin: instead of
        // converting the timestamp into nanos it simply returned the nanoseconds
        // part of the timestamp.
        //
        // We can't fix the behavior because the return type is too small to fit
        // a useful value for nanoseconds since the epoch.
        self.nanos
    }
}

/// A counter that can be used by version 1 and version 6 UUIDs to support
/// the uniqueness of timestamps.
///
/// # References
///
/// * [Clock Sequence in RFC4122](https://datatracker.ietf.org/doc/html/rfc4122#section-4.1.5)
pub trait ClockSequence {
    /// The type of sequence returned by this counter.
    type Output;

    /// Get the next value in the sequence to feed into a timestamp.
    ///
    /// This method will be called each time a [`Timestamp`] is constructed.
    fn generate_sequence(&self, seconds: u64, subsec_nanos: u32) -> Self::Output;
}

impl<'a, T: ClockSequence + ?Sized> ClockSequence for &'a T {
    type Output = T::Output;
    fn generate_sequence(&self, seconds: u64, subsec_nanos: u32) -> Self::Output {
        (**self).generate_sequence(seconds, subsec_nanos)
    }
}

/// Default implementations for the [`ClockSequence`] trait.
pub mod context {
    use super::ClockSequence;

    #[cfg(any(feature = "v1", feature = "v6"))]
    use private_atomic::{Atomic, Ordering};

    /// An empty counter that will always return the value `0`.
    ///
    /// This type should be used when constructing timestamps for version 7 UUIDs,
    /// since they don't need a counter for uniqueness.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct NoContext;

    impl ClockSequence for NoContext {
        type Output = u16;

        fn generate_sequence(&self, _seconds: u64, _nanos: u32) -> Self::Output {
            0
        }
    }

    #[cfg(all(any(feature = "v1", feature = "v6"), feature = "std", feature = "rng"))]
    static CONTEXT: Context = Context {
        count: Atomic::new(0),
    };

    #[cfg(all(any(feature = "v1", feature = "v6"), feature = "std", feature = "rng"))]
    static CONTEXT_INITIALIZED: Atomic<bool> = Atomic::new(false);

    #[cfg(all(any(feature = "v1", feature = "v6"), feature = "std", feature = "rng"))]
    pub(crate) fn shared_context() -> &'static Context {
        // If the context is in its initial state then assign it to a random value
        // It doesn't matter if multiple threads observe `false` here and initialize the context
        if CONTEXT_INITIALIZED
            .compare_exchange(false, true, Ordering::Relaxed, Ordering::Relaxed)
            .is_ok()
        {
            CONTEXT.count.store(crate::rng::u16(), Ordering::Release);
        }

        &CONTEXT
    }

    /// A thread-safe, wrapping counter that produces 14-bit numbers.
    ///
    /// This type should be used when constructing version 1 and version 6 UUIDs.
    #[derive(Debug)]
    #[cfg(any(feature = "v1", feature = "v6"))]
    pub struct Context {
        count: Atomic<u16>,
    }

    #[cfg(any(feature = "v1", feature = "v6"))]
    impl Context {
        /// Construct a new context that's initialized with the given value.
        ///
        /// The starting value should be a random number, so that UUIDs from
        /// different systems with the same timestamps are less likely to collide.
        /// When the `rng` feature is enabled, prefer the [`Context::new_random`] method.
        pub const fn new(count: u16) -> Self {
            Self {
                count: Atomic::<u16>::new(count),
            }
        }

        /// Construct a new context that's initialized with a random value.
        #[cfg(feature = "rng")]
        pub fn new_random() -> Self {
            Self {
                count: Atomic::<u16>::new(crate::rng::u16()),
            }
        }
    }

    #[cfg(any(feature = "v1", feature = "v6"))]
    impl ClockSequence for Context {
        type Output = u16;

        fn generate_sequence(&self, _seconds: u64, _nanos: u32) -> Self::Output {
            // RFC4122 reserves 2 bits of the clock sequence so the actual
            // maximum value is smaller than `u16::MAX`. Since we unconditionally
            // increment the clock sequence we want to wrap once it becomes larger
            // than what we can represent in a "u14". Otherwise there'd be patches
            // where the clock sequence doesn't change regardless of the timestamp
            self.count.fetch_add(1, Ordering::AcqRel) % (u16::MAX >> 2)
        }
    }
}
