//! The hybrid logical clock (ADR 0006, section 2): 48 bits of Unix milliseconds and a 16-bit
//! counter in one `u64`. Never behind anything the device has seen, whatever its wall clock does.

/// Bits of the counter. A counter that overflows carries into the milliseconds, which keeps the
/// clock strictly increasing.
const COUNTER_BITS: u32 = 16;
/// Largest millisecond value whose shifted clock still fits SQLite's signed 64-bit integer.
const MAX_MS: i64 = (1 << 47) - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Hlc(u64);

impl Hlc {
    pub const ZERO: Self = Self(0);

    pub fn from_parts(unix_ms: i64, counter: u16) -> Self {
        let ms = unix_ms.clamp(0, MAX_MS).unsigned_abs();
        Self((ms << COUNTER_BITS) | u64::from(counter))
    }

    /// From the integer SQLite stores. A negative value cannot come from `to_stored`, so it is
    /// treated as the start of time.
    pub fn from_stored(value: i64) -> Self {
        Self(u64::try_from(value).unwrap_or(0))
    }

    pub fn to_stored(self) -> i64 {
        i64::try_from(self.0).unwrap_or(i64::MAX)
    }

    pub fn unix_ms(self) -> u64 {
        self.0 >> COUNTER_BITS
    }

    pub fn counter(self) -> u16 {
        (self.0 & 0xffff) as u16
    }

    /// The stamp for a local write at wall time `now_ms`: `max(now, last + 1)`.
    #[must_use]
    pub fn next(self, now_ms: i64) -> Self {
        let wall = Self::from_parts(now_ms, 0);
        wall.max(Self(self.0.saturating_add(1)))
    }

    /// After receiving a change: never behind what was seen.
    #[must_use]
    pub fn observe(self, received: Self) -> Self {
        self.max(received)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_wall_clock_when_it_moves_forward() {
        let hlc = Hlc::ZERO.next(1_000);
        assert_eq!((hlc.unix_ms(), hlc.counter()), (1_000, 0));
        let later = hlc.next(2_000);
        assert_eq!((later.unix_ms(), later.counter()), (2_000, 0));
    }

    #[test]
    fn counts_when_the_wall_clock_stands_still() {
        let first = Hlc::ZERO.next(1_000);
        let second = first.next(1_000);
        let third = second.next(1_000);
        assert_eq!((third.unix_ms(), third.counter()), (1_000, 2));
        assert!(first < second && second < third);
    }

    #[test]
    fn never_runs_backwards_when_the_wall_clock_does() {
        let first = Hlc::ZERO.next(5_000);
        let second = first.next(1_000);
        assert!(second > first);
        assert_eq!(second.unix_ms(), 5_000);
    }

    #[test]
    fn a_full_counter_carries_into_the_milliseconds() {
        let full = Hlc::from_parts(1_000, u16::MAX);
        let next = full.next(1_000);
        assert!(next > full);
        assert_eq!((next.unix_ms(), next.counter()), (1_001, 0));
    }

    #[test]
    fn observing_a_change_from_the_future_moves_later_writes_after_it() {
        let ahead = Hlc::from_parts(9_000, 3);
        let last = Hlc::from_parts(1_000, 0).observe(ahead);
        assert_eq!(last, ahead);
        assert!(last.next(1_000) > ahead);
        // Observing something older changes nothing.
        assert_eq!(last.observe(Hlc::from_parts(10, 0)), ahead);
    }

    #[test]
    fn stored_form_round_trips() {
        let hlc = Hlc::from_parts(1_700_000_000_000, 42);
        assert_eq!(Hlc::from_stored(hlc.to_stored()), hlc);
        assert!(hlc.to_stored() > 0);
        assert_eq!(Hlc::from_stored(-1), Hlc::ZERO);
    }

    #[test]
    fn absurd_wall_times_stay_storable() {
        let hlc = Hlc::ZERO.next(i64::MAX);
        assert!(hlc.to_stored() > 0);
        assert_eq!(Hlc::ZERO.next(i64::MIN).unix_ms(), 0);
    }
}
