//! Stable-interface selection and interval accounting, independent of Win32.
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    All,
    Manual,
}

#[derive(Clone, Debug, Default)]
pub struct Interface {
    pub id: u64,
    pub received: u64,
    pub sent: u64,
    pub up: bool,
    pub loopback: bool,
    /// IP Helper address enumeration excludes WFP/QoS filter interfaces.
    pub eligible: bool,
    pub physical: bool,
    pub gateway: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub received_delta: u64,
    pub sent_delta: u64,
    pub received_per_second: u64,
    pub sent_per_second: u64,
    pub selected: BTreeSet<u64>,
}

#[derive(Default)]
pub struct Selector {
    mode: Mode,
    manual: BTreeSet<u64>,
    selected: BTreeSet<u64>,
    previous: BTreeMap<u64, (u64, u64)>,
    idle_ms: u64,
}

impl Selector {
    pub fn configure(&mut self, mode: Mode, manual: BTreeSet<u64>) {
        if self.mode != mode || self.manual != manual {
            self.mode = mode;
            self.manual = manual;
            self.selected.clear();
            self.previous.clear();
            self.idle_ms = 0;
        }
    }
    pub fn update(&mut self, interfaces: &[Interface], elapsed_ms: u64) -> Reading {
        let eligible: Vec<_> = interfaces
            .iter()
            .filter(|i| i.up && !i.loopback && i.eligible)
            .collect();
        let delta = |i: &Interface| -> (u64, u64) {
            match self.previous.get(&i.id) {
                Some(&(rx, tx)) if (rx != 0 || tx != 0) && i.received >= rx && i.sent >= tx => {
                    (i.received - rx, i.sent - tx)
                }
                _ => (0, 0),
            }
        };
        let mut selected: BTreeSet<u64> = match self.mode {
            Mode::All => eligible.iter().map(|i| i.id).collect(),
            Mode::Manual => interfaces
                .iter()
                .filter(|i| i.up && !i.loopback && self.manual.contains(&i.id))
                .map(|i| i.id)
                .collect(),
            Mode::Auto => {
                let current = eligible.iter().find(|i| self.selected.contains(&i.id));
                if let Some(current) = current {
                    BTreeSet::from([current.id])
                } else {
                    eligible
                        .iter()
                        .max_by_key(|i| (i.gateway, i.physical, std::cmp::Reverse(i.id)))
                        .map(|i| BTreeSet::from([i.id]))
                        .unwrap_or_default()
                }
            }
        };
        if self.mode == Mode::Auto {
            let active = eligible
                .iter()
                .filter(|i| selected.contains(&i.id))
                .any(|i| {
                    let (r, t) = delta(i);
                    r != 0 || t != 0
                });
            self.idle_ms = if active {
                0
            } else {
                self.idle_ms.saturating_add(elapsed_ms)
            };
            if self.idle_ms >= 30_000 {
                if let Some(candidate) = eligible
                    .iter()
                    .filter(|i| {
                        let (r, t) = delta(i);
                        r != 0 || t != 0
                    })
                    .max_by_key(|i| {
                        let (r, t) = delta(i);
                        u128::from(r) + u128::from(t)
                    })
                {
                    selected = BTreeSet::from([candidate.id]);
                    self.idle_ms = 0;
                }
            }
        }
        let mut received_delta = 0u64;
        let mut sent_delta = 0u64;
        for i in interfaces.iter().filter(|i| selected.contains(&i.id)) {
            // A newly selected connection must establish its own baseline.
            if !self.selected.contains(&i.id) {
                continue;
            }
            let (rx, tx) = delta(i);
            received_delta = received_delta.saturating_add(rx);
            sent_delta = sent_delta.saturating_add(tx);
        }
        self.previous = interfaces
            .iter()
            .filter(|i| i.up && !i.loopback)
            .map(|i| (i.id, (i.received, i.sent)))
            .collect();
        self.selected = selected.clone();
        let rate = |bytes: u64| {
            if elapsed_ms == 0 {
                0
            } else {
                (u128::from(bytes) * 1000 / u128::from(elapsed_ms)).min(u128::from(u64::MAX)) as u64
            }
        };
        Reading {
            received_delta,
            sent_delta,
            received_per_second: rate(received_delta),
            sent_per_second: rate(sent_delta),
            selected,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn iface(id: u64, rx: u64, physical: bool, gateway: bool) -> Interface {
        Interface {
            id,
            received: rx,
            sent: 100,
            up: true,
            eligible: true,
            physical,
            gateway,
            ..Default::default()
        }
    }
    #[test]
    fn auto_prefers_default_physical_and_does_not_double_count_filters() {
        let mut selector = Selector::default();
        let mut filter = iface(2, 1000, true, true);
        filter.eligible = false;
        selector.update(
            &[
                iface(1, 1000, true, true),
                filter.clone(),
                iface(3, 5000, false, false),
            ],
            1000,
        );
        let result = selector.update(
            &[
                iface(1, 2000, true, true),
                Interface {
                    received: 2000,
                    ..filter
                },
                iface(3, 9000, false, false),
            ],
            1000,
        );
        assert_eq!(result.selected, BTreeSet::from([1]));
        assert_eq!(result.received_delta, 1000);
    }
    #[test]
    fn all_means_eligible_network_connections_not_every_filter_row() {
        let mut selector = Selector::default();
        selector.configure(Mode::All, BTreeSet::new());
        let mut filter = iface(3, 1000, true, true);
        filter.eligible = false;
        selector.update(
            &[
                iface(1, 1000, true, true),
                iface(2, 1000, false, false),
                filter.clone(),
            ],
            1000,
        );
        let result = selector.update(
            &[
                iface(1, 2000, true, true),
                iface(2, 4000, false, false),
                Interface {
                    received: 9000,
                    ..filter
                },
            ],
            2000,
        );
        assert_eq!(result.received_delta, 4000);
        assert_eq!(result.received_per_second, 2000);
        assert_eq!(result.selected.len(), 2);
    }
    #[test]
    fn new_removed_and_reconnected_interfaces_do_not_create_lifetime_spikes() {
        let mut s = Selector::default();
        s.configure(Mode::All, BTreeSet::new());
        assert_eq!(
            s.update(&[iface(1, 5000, true, true)], 1000).received_delta,
            0
        );
        assert_eq!(s.update(&[], 1000).received_delta, 0);
        assert_eq!(
            s.update(&[iface(1, 9000, true, true)], 1000).received_delta,
            0
        );
        assert_eq!(
            s.update(
                &[iface(1, 9500, true, true), iface(2, 999_999, false, false)],
                1000
            )
            .received_delta,
            500
        );
    }
    #[test]
    fn manual_selection_survives_enumeration_reordering() {
        let mut s = Selector::default();
        s.configure(Mode::Manual, BTreeSet::from([2]));
        s.update(
            &[iface(1, 1000, true, true), iface(2, 5000, false, false)],
            1000,
        );
        let result = s.update(
            &[iface(2, 6000, false, false), iface(1, 9000, true, true)],
            500,
        );
        assert_eq!(result.received_delta, 1000);
        assert_eq!(result.received_per_second, 2000);
    }
    #[test]
    fn switching_modes_resets_baseline_and_counter_rollback_is_discarded() {
        let mut s = Selector::default();
        s.update(&[iface(1, 1000, true, true)], 1000);
        s.configure(Mode::Manual, BTreeSet::from([1]));
        assert_eq!(
            s.update(&[iface(1, 9000, true, true)], 1000).received_delta,
            0
        );
        assert_eq!(
            s.update(&[iface(1, 100, true, true)], 1000).received_delta,
            0
        );
        assert_eq!(
            s.update(&[iface(1, 200, true, true)], 1000).received_delta,
            100
        );
    }
    #[test]
    fn idle_auto_selection_can_follow_an_active_connection_without_counting_old_bytes() {
        let mut s = Selector::default();
        s.update(
            &[iface(1, 1000, true, true), iface(2, 1000, false, false)],
            1000,
        );
        for i in 1..=30 {
            s.update(
                &[
                    iface(1, 1000, true, true),
                    iface(2, 1000 + i * 100, false, false),
                ],
                1000,
            );
        }
        let result = s.update(
            &[iface(1, 1000, true, true), iface(2, 4100, false, false)],
            1000,
        );
        assert_eq!(result.selected, BTreeSet::from([2]));
        assert!(result.received_delta <= 100);
    }
}
