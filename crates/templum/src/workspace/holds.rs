//! What the person changed on a remote workspace and sees at once, standing over home's state
//! until home's shows it or [`HOLD`] passes (docs/specs/templum-to-templum.md, The canvas ▸ Holds).

use std::collections::HashMap;
use std::hash::Hash;
use std::time::{Duration, Instant};

/// How long a change made here stands over home's, if home's state doesn't show it before.
pub(crate) const HOLD: Duration = Duration::from_secs(2);

/// What was changed here and not yet shown by home, by what it is about: a block moved, and from
/// 2b on a block closed, a wire, a card moved, an item checked. Each stands over home's state
/// until home's shows it or [`HOLD`] passes.
pub(crate) struct Holds<K, V> {
    held: HashMap<K, (V, Instant)>,
}

impl<K, V> Default for Holds<K, V> {
    fn default() -> Self {
        Holds {
            held: HashMap::new(),
        }
    }
}

impl<K: Copy + Eq + Hash, V: Clone> Holds<K, V> {
    /// `value` stands over home's for `key` from `now`.
    pub fn hold(&mut self, key: K, value: V, now: Instant) {
        self.held.insert(key, (value, now));
    }

    /// What was sent for `key` and not yet shown by home.
    pub fn get(&self, key: &K) -> Option<&V> {
        self.held.get(key).map(|(value, _)| value)
    }

    /// Every key held, in no order.
    pub fn keys(&self) -> Vec<K> {
        self.held.keys().copied().collect()
    }

    /// A refusal, or the thing gone: the hold ends at once.
    pub fn end(&mut self, key: &K) -> Option<V> {
        self.held.remove(key).map(|(value, _)| value)
    }

    /// What to show for `key`, home having `home`: the value held while it is younger than
    /// [`HOLD`] and `same` says home's isn't it yet; else home's, and the hold ends.
    pub fn shown(&mut self, key: K, home: V, now: Instant, same: impl Fn(&V, &V) -> bool) -> V {
        match self.held.get(&key) {
            Some((mine, at)) if !same(mine, &home) && now.saturating_duration_since(*at) < HOLD => {
                mine.clone()
            }
            Some(_) => {
                self.held.remove(&key);
                home
            }
            None => home,
        }
    }

    /// The holds older than [`HOLD`] end, except those `kept` keeps (a block in the person's
    /// hand isn't taken from it; its release holds it again): the keys that ended, and when the
    /// next one is due.
    pub fn expire(&mut self, now: Instant, kept: impl Fn(&K) -> bool) -> (Vec<K>, Option<Instant>) {
        let (mut ended, mut due) = (Vec::new(), None::<Instant>);
        self.held.retain(|key, (_, at)| {
            if kept(key) {
                return true;
            }
            let ends = *at + HOLD;
            if now >= ends {
                ended.push(*key);
                false
            } else {
                due = Some(due.map_or(ends, |d| d.min(ends)));
                true
            }
        });
        (ended, due)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hold_stands_until_home_shows_it_or_hold_passes() {
        let t = Instant::now();
        let mut holds = Holds::<u64, u64>::default();
        holds.hold(1, 10, t);
        let secs = Duration::from_secs_f64;
        assert_eq!(holds.shown(1, 5, t + secs(1.0), |a, b| a == b), 10);
        assert_eq!(holds.shown(1, 10, t + secs(1.5), |a, b| a == b), 10);
        assert_eq!(holds.get(&1), None, "home showed it");
        holds.hold(2, 20, t);
        assert_eq!(holds.shown(2, 5, t + HOLD, |a, b| a == b), 5);
        assert_eq!(holds.get(&2), None);
    }

    #[test]
    fn keys_lists_what_is_held() {
        let t = Instant::now();
        let mut holds = Holds::<u64, u64>::default();
        holds.hold(1, 10, t);
        holds.hold(2, 20, t);
        holds.end(&1);
        assert_eq!(holds.keys(), vec![2]);
    }

    #[test]
    fn a_refusal_ends_a_hold_at_once() {
        let t = Instant::now();
        let mut holds = Holds::<u64, u64>::default();
        holds.hold(1, 10, t);
        assert_eq!(holds.end(&1), Some(10));
        assert_eq!(holds.shown(1, 5, t, |a, b| a == b), 5);
    }

    #[test]
    fn expire_spares_what_is_kept_and_says_when_the_next_is_due() {
        let t = Instant::now();
        let mut holds = Holds::<u64, u64>::default();
        holds.hold(1, 10, t);
        holds.hold(2, 20, t + Duration::from_secs(1));
        holds.hold(3, 30, t);
        let (ended, due) = holds.expire(t + HOLD, |key| *key == 3);
        assert_eq!(ended, vec![1]);
        assert_eq!(due, Some(t + Duration::from_secs(1) + HOLD));
        assert_eq!(holds.get(&3), Some(&30));
    }
}
