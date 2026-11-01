//! Finds the mod behind a crash by testing shrinking groups, so a 300 mod pack
//! takes around a dozen launches instead of an evening of guessing.
//!
//! Each step says which mods to switch on. A mod's dependencies always come
//! with it, otherwise a test would fail for the wrong reason. Most culprits are
//! a single mod, but some crashes need two mods together. Those show up when
//! both halves work on their own, and the search then finds each half of the pair.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Crashes,
    Works,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase"
)]
pub enum Step {
    Test {
        enabled: Vec<String>,
        disabled: Vec<String>,
        round: u32,
        estimate: u32,
    },
    Found {
        culprits: Vec<String>,
    },
    NotAMod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
enum Stage {
    /// Everything off, to prove the crash comes from a mod at all.
    Baseline,
    /// First half under test, second half waiting.
    FirstHalf {
        rest: Vec<String>,
    },
    /// First half passed, so the second half is under test. `first` holds
    /// every suspect from before the split.
    SecondHalf {
        first: Vec<String>,
    },
    Done(Vec<String>),
    NotAMod,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bisect {
    all: Vec<String>,
    requires: BTreeMap<String, Vec<String>>,
    suspects: Vec<String>,
    /// Kept switched on throughout, while hunting the partner of a pair.
    anchored: Vec<String>,
    /// The other half to search once the first culprit of a pair is found.
    partner_pool: Vec<String>,
    found: Vec<String>,
    trial: Vec<String>,
    stage: Stage,
    round: u32,
}

impl Bisect {
    pub fn new(mods: Vec<String>, requires: BTreeMap<String, Vec<String>>) -> Self {
        Bisect {
            suspects: mods.clone(),
            all: mods,
            requires,
            anchored: Vec::new(),
            partner_pool: Vec::new(),
            found: Vec::new(),
            trial: Vec::new(),
            stage: Stage::Baseline,
            round: 1,
        }
    }

    /// The current instruction.
    pub fn step(&self) -> Step {
        match &self.stage {
            Stage::Done(culprits) => Step::Found {
                culprits: culprits.clone(),
            },
            Stage::NotAMod => Step::NotAMod,
            _ => {
                let enabled = self.closure();
                let disabled = self
                    .all
                    .iter()
                    .filter(|m| !enabled.contains(*m))
                    .cloned()
                    .collect();
                Step::Test {
                    enabled: enabled.into_iter().collect(),
                    disabled,
                    round: self.round,
                    estimate: self.round + rounds_left(self.suspects.len()),
                }
            }
        }
    }

    /// Records a test result and works out the next test.
    pub fn report(&mut self, verdict: Verdict) -> Step {
        self.round += 1;
        match (std::mem::replace(&mut self.stage, Stage::NotAMod), verdict) {
            (Stage::Baseline, Verdict::Crashes) => self.stage = Stage::NotAMod,
            (Stage::Baseline, Verdict::Works) => self.split(),
            (Stage::FirstHalf { .. }, Verdict::Crashes)
            | (Stage::SecondHalf { .. }, Verdict::Crashes) => {
                self.suspects = std::mem::take(&mut self.trial);
                self.split();
            }
            (Stage::FirstHalf { rest }, Verdict::Works) => {
                self.trial = rest;
                self.stage = Stage::SecondHalf {
                    first: self.suspects.clone(),
                };
            }
            (Stage::SecondHalf { first }, Verdict::Works) => {
                // Each half works alone, so the crash needs something from both.
                let second: Vec<String> = self
                    .suspects
                    .iter()
                    .filter(|m| !first_half_of(&first, m))
                    .cloned()
                    .collect();
                self.partner_pool = second.clone();
                self.anchored.extend(second);
                self.suspects = first
                    .iter()
                    .take(first.len().div_ceil(2))
                    .cloned()
                    .collect();
                self.split();
            }
            (done @ (Stage::Done(_) | Stage::NotAMod), _) => self.stage = done,
        }
        self.step()
    }

    fn split(&mut self) {
        if self.suspects.len() <= 1 {
            let culprit = self.suspects.pop();
            self.found.extend(culprit);
            if self.partner_pool.is_empty() {
                self.stage = Stage::Done(self.found.clone());
            } else {
                self.suspects = std::mem::take(&mut self.partner_pool);
                self.anchored = self.found.clone();
                self.split();
            }
            return;
        }
        let middle = self.suspects.len().div_ceil(2);
        self.trial = self.suspects[..middle].to_vec();
        self.stage = Stage::FirstHalf {
            rest: self.suspects[middle..].to_vec(),
        };
    }

    /// The mods on trial plus everything they need.
    fn closure(&self) -> BTreeSet<String> {
        let mut enabled = BTreeSet::new();
        let mut queue: Vec<&String> = self.trial.iter().chain(&self.anchored).collect();
        while let Some(name) = queue.pop() {
            if enabled.insert(name.clone())
                && let Some(needs) = self.requires.get(name)
            {
                queue.extend(needs);
            }
        }
        enabled
    }
}

fn first_half_of(first: &[String], name: &str) -> bool {
    first
        .iter()
        .take(first.len().div_ceil(2))
        .any(|m| m == name)
}

fn rounds_left(suspects: usize) -> u32 {
    (suspects.max(1) as f64).log2().ceil() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|i| format!("mod-{i}")).collect()
    }

    /// Runs a search against a simulated game and counts the launches.
    fn run(mut search: Bisect, crashes: impl Fn(&[String]) -> bool) -> (Step, u32) {
        let mut launches = 0;
        let mut step = search.step();
        while let Step::Test { enabled, .. } = &step {
            launches += 1;
            assert!(launches < 40, "search did not converge");
            let verdict = if crashes(enabled) {
                Verdict::Crashes
            } else {
                Verdict::Works
            };
            step = search.report(verdict);
        }
        (step, launches)
    }

    #[test]
    fn starts_by_testing_with_every_mod_off() {
        let search = Bisect::new(names(4), BTreeMap::new());
        let Step::Test {
            enabled, disabled, ..
        } = search.step()
        else {
            panic!()
        };
        assert!(enabled.is_empty());
        assert_eq!(disabled.len(), 4);
    }

    #[test]
    fn reports_when_the_crash_is_not_caused_by_a_mod() {
        let (step, launches) = run(Bisect::new(names(8), BTreeMap::new()), |_| true);
        assert_eq!(step, Step::NotAMod);
        assert_eq!(launches, 1);
    }

    #[test]
    fn finds_every_possible_single_culprit() {
        for culprit in 0..33 {
            let target = format!("mod-{culprit}");
            let (step, _) = run(Bisect::new(names(33), BTreeMap::new()), |on| {
                on.contains(&target)
            });
            assert_eq!(
                step,
                Step::Found {
                    culprits: vec![target.clone()]
                }
            );
        }
    }

    #[test]
    fn a_large_pack_needs_few_launches() {
        let (step, launches) = run(Bisect::new(names(300), BTreeMap::new()), |on| {
            on.contains(&"mod-217".to_string())
        });
        assert_eq!(
            step,
            Step::Found {
                culprits: vec!["mod-217".to_string()]
            }
        );
        assert!(launches <= 18, "took {launches} launches");
    }

    #[test]
    fn finds_a_pair_that_only_crashes_together() {
        let (step, _) = run(Bisect::new(names(16), BTreeMap::new()), |on| {
            on.contains(&"mod-2".to_string()) && on.contains(&"mod-11".to_string())
        });
        let Step::Found { mut culprits } = step else {
            panic!("expected culprits, got {step:?}")
        };
        culprits.sort();
        assert_eq!(culprits, ["mod-11", "mod-2"]);
    }

    #[test]
    fn dependencies_stay_on_with_the_mods_that_need_them() {
        let mut search = Bisect::new(
            vec![
                "iris".into(),
                "sodium".into(),
                "other".into(),
                "more".into(),
            ],
            BTreeMap::from([("iris".to_string(), vec!["sodium".to_string()])]),
        );
        search.suspects = vec!["iris".into(), "other".into()];
        let Step::Test { enabled, .. } = search.report(Verdict::Works) else {
            panic!()
        };
        assert!(enabled.contains(&"iris".to_string()));
        assert!(enabled.contains(&"sodium".to_string()));
    }
}
