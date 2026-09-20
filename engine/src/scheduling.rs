//! Scheduling is server-owned and driven by an injectable clock.
use crate::{monitoring::Schedule, pipelines::Pipelines, store::Result};
use chrono::{DateTime, Datelike, LocalResult, NaiveTime, TimeZone, Utc};
use std::hash::{Hash, Hasher};
impl Schedule {
    pub fn next_after(&self, after: i64) -> Result<i64> {
        match self {
            Self::Interval { every_ms } => after
                .checked_add(*every_ms as i64)
                .ok_or_else(|| "schedule overflow".into()),
            Self::Daily {
                time,
                zone,
                weekdays,
            } => {
                let zone = zone.parse::<chrono_tz::Tz>().map_err(|_| "time zone")?;
                let time = NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| "calendar time")?;
                let start = DateTime::<Utc>::from_timestamp_millis(after)
                    .ok_or("calendar range")?
                    .with_timezone(&zone)
                    .date_naive();
                for offset in 0..10 {
                    let day = start
                        .checked_add_signed(chrono::Duration::days(offset))
                        .ok_or("calendar overflow")?;
                    if !weekdays.is_empty()
                        && !weekdays.contains(&day.weekday().number_from_monday())
                    {
                        continue;
                    }
                    let mut local = day.and_time(time);
                    for _ in 0..2881 {
                        let candidate = match zone.from_local_datetime(&local) {
                            LocalResult::Single(t) => Some(t.timestamp_millis()),
                            LocalResult::Ambiguous(a, b) => {
                                Some(a.timestamp_millis().min(b.timestamp_millis()))
                            }
                            LocalResult::None => None,
                        };
                        if let Some(t) = candidate {
                            if t > after {
                                return Ok(t);
                            }
                            break;
                        }
                        local = local
                            .checked_add_signed(chrono::Duration::minutes(1))
                            .ok_or("calendar overflow")?;
                    }
                }
                Err("calendar next occurrence unavailable".into())
            }
        }
    }
    pub fn advance(&self, due: i64, now: i64) -> Result<(i64, u64)> {
        if due > now {
            return Ok((due, 0));
        }
        match self {
            Self::Interval { every_ms } => {
                let occurrences = ((now as i128 - due as i128) / (*every_ms as i128) + 1) as u64;
                let next = due as i128 + occurrences as i128 * (*every_ms as i128);
                Ok((
                    i64::try_from(next).map_err(|_| "schedule overflow")?,
                    occurrences,
                ))
            }
            _ => {
                let mut next = due;
                let mut n = 0;
                while next <= now {
                    next = self.next_after(next)?;
                    n += 1;
                    if n > 40000 {
                        return Err("calendar recovery range exceeded".into());
                    }
                }
                Ok((next, n))
            }
        }
    }
}
pub fn request_key(prefix: &str, instance: &str, generation: u64, sequence: i64) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    instance.hash(&mut h);
    format!("{prefix}-{:016x}-{generation}-{sequence}", h.finish())
}
#[derive(Clone)]
pub struct Scheduler {
    pub pipelines: Pipelines,
}
impl Scheduler {
    pub fn tick(&self) -> Result<()> {
        let e = &self.pipelines.engine;
        let now = e.now();
        let config = e.store.borrow().monitoring_config()?;
        for i in &config.instances {
            if !i.enabled {
                continue;
            }
            if config.definition(&i.definition)?.kind != "pipeline" {
                continue;
            }
            if let Some(schedule) = &i.schedule {
                let mut s = e.store.borrow_mut();
                let mut state = s.monitor_state(&i.id)?;
                let due = state.next_due.unwrap_or(match schedule {
                    Schedule::Interval { .. } => now,
                    _ => schedule.next_after(now)?,
                });
                if due > now {
                    if state.next_due.is_none() {
                        state.next_due = Some(due);
                        s.save_monitor(&state)?;
                    }
                } else {
                    let (next, occurrences) = schedule.advance(due, now)?;
                    let missed = occurrences.saturating_sub(1);
                    let request = request_key("schedule", &i.id, state.generation, due);
                    s.monitoring_atomic(|s| {
                        let admitted = s.admit(
                            &i.id,
                            &request,
                            if missed > 0 { "recovery" } else { "scheduled" },
                            0,
                            now,
                        )?;
                        state.next_due = Some(next);
                        state.last_due = Some(due);
                        state.missed = state
                            .missed
                            .saturating_add(missed + u64::from(admitted.disposition == "skipped"));
                        if missed > 0 {
                            state.last_gap = Some((due, now));
                        }
                        s.save_monitor(&state)
                    })?;
                }
            }
            if let Some(max_age) = i.stale_after_ms {
                for output in i.outputs.values() {
                    let mut s = e.store.borrow_mut();
                    let mut v = s.instance(output)?;
                    if v.quality == "good"
                        && (!v.has_value || now.saturating_sub(v.timestamp) > max_age as i64)
                    {
                        let expected = v.revision;
                        v.revision += 1;
                        v.quality = "stale".into();
                        s.commit(expected, &v, false, now)?;
                        drop(s);
                        e.changed(output);
                    }
                }
            }
        }
        self.pipelines.pump()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn ts(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }
    #[test]
    fn calendar_dst_gap_fold_weekdays() {
        let s = Schedule::Daily {
            time: "02:30".into(),
            zone: "Europe/Rome".into(),
            weekdays: vec![],
        };
        assert_eq!(
            s.next_after(ts("2026-03-28T03:00:00+01:00")).unwrap(),
            ts("2026-03-29T03:00:00+02:00")
        );
        let first = s.next_after(ts("2026-10-24T03:00:00+02:00")).unwrap();
        assert_eq!(first, ts("2026-10-25T02:30:00+02:00"));
        assert_eq!(
            s.next_after(first).unwrap(),
            ts("2026-10-26T02:30:00+01:00")
        );
        let weekly = Schedule::Daily {
            time: "09:00".into(),
            zone: "Europe/Rome".into(),
            weekdays: vec![1],
        };
        assert_eq!(
            weekly.next_after(ts("2026-09-18T10:00:00+02:00")).unwrap(),
            ts("2026-09-21T09:00:00+02:00")
        );
    }
    #[tokio::test]
    async fn missed_runs_and_freshness_without_clients() {
        tokio::task::LocalSet::new()
            .run_until(async {
                use std::{cell::Cell, rc::Rc};
                let mut s = crate::store::Store::open(":memory:").unwrap();
                crate::store::tests::seed(&mut s);
                let mut c = crate::monitoring::tests::config();
                c.definitions[0].source = "{async run(ctx){await ctx.publish('metric',1)}}".into();
                c.instances[0].schedule = Some(Schedule::Interval { every_ms: 1000 });
                c.instances[0].stale_after_ms = Some(500);
                s.configure_monitoring(&c, 0).unwrap();
                let now = Rc::new(Cell::new(1000));
                let clock = now.clone();
                let e = crate::runtime::Engine::with_clock(s, Rc::new(move || clock.get()));
                let p = Pipelines::new(e.clone()).unwrap();
                let scheduler = Scheduler {
                    pipelines: p.clone(),
                };
                scheduler.tick().unwrap();
                let id = e.store.borrow().runs().unwrap()[0].id.clone();
                assert_eq!(
                    crate::pipelines::tests::finish(&p, &id).await.status,
                    "complete"
                );
                assert_eq!(e.store.borrow().instance("metric").unwrap().timestamp, 1000);
                now.set(1600);
                scheduler.tick().unwrap();
                assert_eq!(
                    e.store.borrow().instance("metric").unwrap().quality,
                    "stale"
                );
                now.set(62000);
                scheduler.tick().unwrap();
                let id = e.store.borrow().runs().unwrap()[0].id.clone();
                assert_eq!(
                    crate::pipelines::tests::finish(&p, &id).await.status,
                    "complete"
                );
                let state = e.store.borrow().monitor_state("x").unwrap();
                assert_eq!(state.missed, 60);
                assert_eq!(state.next_due, Some(63000));
                assert_eq!(state.last_gap, Some((2000, 62000)));
                let v = e.store.borrow().instance("metric").unwrap();
                assert_eq!(v.timestamp, 62000);
                assert_eq!(v.quality, "good");
                assert_eq!(e.store.borrow().runs().unwrap().len(), 2);
                scheduler.tick().unwrap();
                assert_eq!(e.store.borrow().runs().unwrap().len(), 2);
            })
            .await;
    }
}
