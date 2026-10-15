//! The schedules are checked in a zone with explicit offsets — New York's
//! 2026 rules written out as a [`RuleZone`] — so daylight saving is tested the
//! same way on every machine, whatever zone it is in.

use super::schedule::{resolve, wall};
use super::*;
use crate::workspace::model::Agent;
use crate::workspace::Workspace;

const HOUR: i64 = 3600;
const EST: i64 = -5 * HOUR;
const EDT: i64 = -4 * HOUR;

/// 2026-03-08 02:00 EST → 03:00 EDT; 2026-11-01 02:00 EDT → 01:00 EST.
fn new_york() -> RuleZone {
    RuleZone {
        initial: EST,
        changes: vec![(utc(2026, 3, 8, 7, 0), EDT), (utc(2026, 11, 1, 6, 0), EST)],
    }
}

fn utc(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> Instant {
    wall(year, month, day) * 86_400 + hour * HOUR + minute * 60
}

/// A wall-clock time in a known offset.
fn at(year: i64, month: i64, day: i64, hour: i64, minute: i64, offset: i64) -> Instant {
    utc(year, month, day, hour, minute) - offset
}

fn daily(at: &str) -> Schedule {
    Schedule::Daily { at: at.into() }
}

fn stamp(when: Instant) -> String {
    local_stamp(&new_york(), when)
}

#[test]
fn hhmm_is_strict() {
    assert_eq!(schedule::parse_hhmm("09:05"), Some(9 * 60 + 5));
    assert_eq!(schedule::parse_hhmm("23:59"), Some(23 * 60 + 59));
    for bad in [
        "9:05", "24:00", "12:60", "ab:cd", "", "12:5", "12:05:00", "+1:00",
    ] {
        assert_eq!(schedule::parse_hhmm(bad), None, "{bad}");
    }
}

#[test]
fn weekdays_are_counted_from_a_thursday() {
    assert_eq!(Day::of(0), Day::Thu);
    assert_eq!(Day::of(wall(2026, 6, 12)), Day::Fri);
    assert_eq!(Day::of(wall(2026, 6, 15)), Day::Mon);
    assert_eq!(Day::of(-1), Day::Wed);
}

#[test]
fn daily_runs_today_then_tomorrow() {
    let zone = new_york();
    let schedule = daily("09:00");
    let before = at(2026, 6, 10, 8, 0, EDT);
    assert_eq!(
        next_run(&schedule, before, &zone),
        Some(at(2026, 6, 10, 9, 0, EDT))
    );
    // Exactly at the time: that one is past.
    assert_eq!(
        next_run(&schedule, at(2026, 6, 10, 9, 0, EDT), &zone),
        Some(at(2026, 6, 11, 9, 0, EDT))
    );
    assert_eq!(
        next_runs(&schedule, before, 3, &zone)
            .into_iter()
            .map(stamp)
            .collect::<Vec<_>>(),
        ["2026-06-10 09:00", "2026-06-11 09:00", "2026-06-12 09:00"]
    );
}

#[test]
fn weekdays_skip_the_days_not_picked() {
    let zone = new_york();
    let schedule = Schedule::Weekdays {
        at: "09:00".into(),
        days: vec![Day::Mon, Day::Tue, Day::Wed, Day::Thu, Day::Fri],
    };
    // Friday after nine: next is Monday.
    assert_eq!(
        next_run(&schedule, at(2026, 6, 12, 10, 0, EDT), &zone).map(stamp),
        Some("2026-06-15 09:00".into())
    );
    let sundays = Schedule::Weekdays {
        at: "18:30".into(),
        days: vec![Day::Sun],
    };
    assert_eq!(
        next_runs(&sundays, at(2026, 6, 15, 0, 0, EDT), 2, &zone)
            .into_iter()
            .map(stamp)
            .collect::<Vec<_>>(),
        ["2026-06-21 18:30", "2026-06-28 18:30"]
    );
}

#[test]
fn every_n_hours_restarts_from_its_time_each_day() {
    let zone = new_york();
    let daytime = Schedule::Every {
        hours: 4,
        from: Some("09:00".into()),
    };
    assert_eq!(
        next_runs(&daytime, at(2026, 6, 10, 8, 0, EDT), 5, &zone)
            .into_iter()
            .map(stamp)
            .collect::<Vec<_>>(),
        [
            "2026-06-10 09:00",
            "2026-06-10 13:00",
            "2026-06-10 17:00",
            "2026-06-10 21:00",
            "2026-06-11 09:00"
        ]
    );
    let round_the_clock = Schedule::Every {
        hours: 6,
        from: None,
    };
    assert_eq!(
        next_runs(&round_the_clock, at(2026, 6, 10, 13, 0, EDT), 3, &zone)
            .into_iter()
            .map(stamp)
            .collect::<Vec<_>>(),
        ["2026-06-10 18:00", "2026-06-11 00:00", "2026-06-11 06:00"]
    );
    let once_a_day = Schedule::Every {
        hours: 24,
        from: Some("07:15".into()),
    };
    assert_eq!(
        next_run(&once_a_day, at(2026, 6, 10, 8, 0, EDT), &zone).map(stamp),
        Some("2026-06-11 07:15".into())
    );
}

/// 02:30 does not exist on 8 March: it runs at 03:00, the first minute
/// that does, and the next day at 02:30 again.
#[test]
fn a_time_in_the_spring_gap_runs_at_the_first_minute_after_it() {
    let zone = new_york();
    let schedule = daily("02:30");
    let first = next_run(&schedule, at(2026, 3, 8, 0, 0, EST), &zone).unwrap();
    assert_eq!(first, at(2026, 3, 8, 3, 0, EDT));
    assert_eq!(stamp(first), "2026-03-08 03:00");
    assert_eq!(
        next_run(&schedule, first, &zone),
        Some(at(2026, 3, 9, 2, 30, EDT))
    );
    // The day before is in standard time.
    assert_eq!(
        next_run(&schedule, at(2026, 3, 7, 0, 0, EST), &zone),
        Some(at(2026, 3, 7, 2, 30, EST))
    );
    // Hourly through the gap: 01:00 EST, then 03:00 EDT, never 02:00.
    let hourly = Schedule::Every {
        hours: 1,
        from: None,
    };
    assert_eq!(
        next_runs(&hourly, at(2026, 3, 8, 0, 30, EST), 3, &zone)
            .into_iter()
            .map(stamp)
            .collect::<Vec<_>>(),
        ["2026-03-08 01:00", "2026-03-08 03:00", "2026-03-08 04:00"]
    );
}

/// 01:30 happens twice on 1 November: it runs once, at the first.
#[test]
fn a_repeated_hour_runs_once() {
    let zone = new_york();
    let schedule = daily("01:30");
    let first = next_run(&schedule, at(2026, 11, 1, 0, 0, EDT), &zone).unwrap();
    assert_eq!(first, at(2026, 11, 1, 1, 30, EDT));
    // Not again an hour later, at 01:30 standard time.
    assert_eq!(
        next_run(&schedule, first, &zone),
        Some(at(2026, 11, 2, 1, 30, EST))
    );
    assert_eq!(resolve(&zone, wall(2026, 11, 1), 90), first);
    let hourly = Schedule::Every {
        hours: 1,
        from: None,
    };
    let runs = next_runs(&hourly, at(2026, 11, 1, 0, 30, EDT), 3, &zone);
    assert_eq!(
        runs,
        [
            at(2026, 11, 1, 1, 0, EDT),
            at(2026, 11, 1, 2, 0, EST),
            at(2026, 11, 1, 3, 0, EST)
        ],
        "two hours apart across the change: 01:00 is not repeated"
    );
}

#[test]
fn the_computers_own_zone_is_sane() {
    let zone = LocalZone;
    let now = utc(2026, 10, 11, 12, 0);
    assert!(zone.offset(now).abs() <= 14 * HOUR);
    let next = next_run(&daily("09:00"), now, &zone).unwrap();
    assert!(next > now && next <= now + 26 * HOUR);
    assert!(local_stamp(&zone, next).ends_with("09:00"));
}

// ---------------------------------------------------------------------------
// Due, catch-up and skipping.
// ---------------------------------------------------------------------------

fn automation(schedule: Schedule, created: Instant) -> Automation {
    Automation {
        id: "a1".into(),
        project_id: "p".into(),
        title: "Morning triage".into(),
        prompt: "Triage the new issues".into(),
        agent: Agent::Claude,
        model: String::new(),
        effort: String::new(),
        profile_id: None,
        view: "chat".into(),
        worktree: false,
        schedule,
        enabled: true,
        catch_up: true,
        created,
        updated: created,
        last_slot: None,
        history: Vec::new(),
        skills: None,
        unknown: Default::default(),
    }
}

#[test]
fn due_once_its_time_comes_and_not_before() {
    let zone = new_york();
    let created = at(2026, 6, 10, 8, 0, EDT);
    let mut item = automation(daily("09:00"), created);
    assert_eq!(due(&item, at(2026, 6, 10, 8, 59, EDT), &zone), None);
    let slot = at(2026, 6, 10, 9, 0, EDT);
    assert_eq!(
        due(&item, slot + 20, &zone),
        Some(Due::Run {
            slot,
            missed: false
        })
    );
    item.last_slot = Some(slot);
    assert_eq!(due(&item, slot + 60, &zone), None, "dealt with");
    item.enabled = false;
    assert_eq!(due(&item, at(2026, 6, 11, 9, 0, EDT), &zone), None);
    assert_eq!(item.next_run(slot, &zone), None, "off: no next run");
}

/// Closed from Monday morning to Thursday noon: three nine o'clocks missed,
/// one run, for the latest.
#[test]
fn missed_runs_catch_up_once() {
    let zone = new_york();
    let mut item = automation(daily("09:00"), at(2026, 6, 8, 8, 0, EDT));
    let now = at(2026, 6, 11, 12, 0, EDT);
    let slot = at(2026, 6, 11, 9, 0, EDT);
    assert_eq!(
        due(&item, now, &zone),
        Some(Due::Run { slot, missed: true })
    );
    assert_eq!(item.next_run(now, &zone), Some(slot), "shown as owed");
    item.last_slot = Some(slot);
    assert_eq!(due(&item, now, &zone), None, "never twice");
    assert_eq!(due(&item, now + 6 * HOUR, &zone), None);
    assert!(matches!(
        due(&item, at(2026, 6, 12, 9, 0, EDT) + 5, &zone),
        Some(Due::Run { missed: false, .. })
    ));

    // Without catch-up the missed one is let go, and the next is normal.
    let mut quiet = automation(daily("09:00"), at(2026, 6, 8, 8, 0, EDT));
    quiet.catch_up = false;
    assert_eq!(due(&quiet, now, &zone), Some(Due::Pass { slot }));
    assert_eq!(
        quiet.next_run(now, &zone),
        Some(at(2026, 6, 12, 9, 0, EDT)),
        "a missed one it will not run is not next"
    );
    // A few minutes late is not missed.
    assert_eq!(
        due(&quiet, at(2026, 6, 8, 9, 4, EDT), &zone),
        Some(Due::Run {
            slot: at(2026, 6, 8, 9, 0, EDT),
            missed: false
        })
    );
}

#[test]
fn an_edit_or_switch_on_owes_nothing_from_before() {
    let zone = new_york();
    let mut item = automation(daily("09:00"), at(2026, 6, 1, 8, 0, EDT));
    item.updated = at(2026, 6, 11, 11, 0, EDT);
    assert_eq!(due(&item, at(2026, 6, 11, 12, 0, EDT), &zone), None);
}

#[test]
fn a_pass_skips_what_is_still_running_and_starts_three_at_most() {
    let zone = new_york();
    let created = at(2026, 6, 10, 8, 0, EDT);
    let now = at(2026, 6, 10, 9, 0, EDT) + 10;
    let all: Vec<Automation> = (0..5)
        .map(|n| {
            let mut item = automation(daily("09:00"), created);
            item.id = format!("a{n}");
            item
        })
        .collect();
    let tick = plan_tick(&all, now, &zone, &|item| item.id == "a1");
    assert_eq!(tick.skip, vec![("a1".to_string(), now - 10)]);
    assert_eq!(
        tick.start
            .iter()
            .map(|(id, ..)| id.as_str())
            .collect::<Vec<_>>(),
        ["a0", "a2", "a3"]
    );
    assert!(tick.start.iter().all(|(_, _, missed)| !missed));
    assert!(tick.passed().is_empty());
    let none = plan_tick(&all, created, &zone, &|_| false);
    assert!(none.is_empty());
}

#[test]
fn names_for_a_run() {
    let zone = new_york();
    let when = at(2026, 10, 11, 9, 5, EDT);
    assert_eq!(
        session_title(" Morning triage ", &zone, when),
        "Morning triage · 2026-10-11 09:05"
    );
    assert_eq!(
        branch_for("Morning triage: CI!", &zone, when),
        "automation/morning-triage-ci-20261011-0905"
    );
    assert_eq!(
        branch_for("Ночной отчёт", &zone, when),
        "automation/run-20261011-0905"
    );
    assert!(crate::helpers::valid_branch(&branch_for(
        "x".repeat(80).as_str(),
        &zone,
        when
    )));
}

// ---------------------------------------------------------------------------
// The file.
// ---------------------------------------------------------------------------

struct World {
    _root: tempfile::TempDir,
    workspace: Workspace,
    project: String,
    store_file: std::path::PathBuf,
}

fn world() -> World {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let storage = crate::Storage::new(root.path().join("state"));
    let mut workspace = Workspace::load(storage.workspace_file()).unwrap();
    workspace.add_project(&folder).unwrap();
    let project = workspace.state().projects[0].id.clone();
    World {
        _root: root,
        workspace,
        project,
        store_file: storage.automations(),
    }
}

fn input(project: &str) -> AutomationInput {
    AutomationInput {
        skills: None,
        id: None,
        project_id: project.into(),
        title: "Morning triage".into(),
        prompt: "Triage the new issues".into(),
        agent: "claude".into(),
        model: "sonnet".into(),
        effort: "high".into(),
        profile_id: None,
        view: "chat".into(),
        worktree: true,
        schedule: Schedule::Weekdays {
            at: "09:00".into(),
            days: vec![Day::Mon, Day::Fri],
        },
        enabled: true,
        catch_up: true,
    }
}

#[test]
fn the_store_round_trips_and_keeps_what_it_does_not_know() {
    let world = world();
    let mut store = Store::load(&world.store_file).unwrap();
    assert!(store.all().is_empty(), "no file is no automations");
    let id = store
        .save(input(&world.project), &world.workspace, 1_000)
        .unwrap();
    store.record_start(&id, "s1", Some(900), 1_000).unwrap();

    // A newer build's keys survive a write by this one.
    let mut raw: Value =
        serde_json::from_str(&fs::read_to_string(&world.store_file).unwrap()).unwrap();
    raw["automations"][0]["future"] = "kept".into();
    raw["future"] = 1.into();
    fs::write(&world.store_file, raw.to_string()).unwrap();

    let mut again = Store::load(&world.store_file).unwrap();
    let item = again.get(&id).unwrap().clone();
    assert_eq!(item.title, "Morning triage");
    assert_eq!(item.agent, Agent::Claude);
    assert_eq!(
        (item.model.as_str(), item.effort.as_str()),
        ("sonnet", "high")
    );
    assert!(item.worktree && item.enabled && item.catch_up);
    assert_eq!(
        item.schedule,
        Schedule::Weekdays {
            at: "09:00".into(),
            days: vec![Day::Mon, Day::Fri]
        }
    );
    assert_eq!(item.last_slot, Some(900));
    assert_eq!(item.last_run().unwrap().status, RunStatus::Started);
    again.set_enabled(&id, false, 2_000).unwrap();
    let text = fs::read_to_string(&world.store_file).unwrap();
    assert!(text.contains("\"future\": \"kept\""), "{text}");
    assert!(text.contains("\"kind\": \"weekdays\""), "{text}");
    assert!(text.contains("\"days\": [\n"), "{text}");

    fs::write(&world.store_file, "{ not json").unwrap();
    let damaged = Store::load(&world.store_file).unwrap_err().to_string();
    assert!(damaged.contains("damaged"), "{damaged}");
}

#[test]
fn editing_keeps_the_record_and_a_new_schedule_starts_fresh() {
    let world = world();
    let mut store = Store::load(&world.store_file).unwrap();
    let id = store
        .save(input(&world.project), &world.workspace, 1_000)
        .unwrap();
    let mut edit = input(&world.project);
    edit.id = Some(id.clone());
    edit.title = "Evening triage".into();
    store.save(edit.clone(), &world.workspace, 5_000).unwrap();
    assert_eq!(store.get(&id).unwrap().updated, 1_000, "same schedule");
    edit.schedule = daily("18:00");
    store.save(edit.clone(), &world.workspace, 6_000).unwrap();
    let item = store.get(&id).unwrap();
    assert_eq!(
        (item.title.as_str(), item.updated, item.created),
        ("Evening triage", 6_000, 1_000)
    );
    assert_eq!(store.all().len(), 1);
    edit.project_id = "elsewhere".into();
    assert!(store.save(edit, &world.workspace, 7_000).is_err());
    store.set_enabled(&id, false, 8_000).unwrap();
    store.set_enabled(&id, true, 9_000).unwrap();
    assert_eq!(store.get(&id).unwrap().updated, 9_000);
    store.remove(&id).unwrap();
    assert!(store.remove(&id).is_err());
}

#[test]
fn validation_names_what_is_wrong() {
    let mut world = world();
    let check = |world: &World, change: &dyn Fn(&mut AutomationInput)| {
        let mut draft = input(&world.project);
        change(&mut draft);
        validate(&draft, &world.workspace)
            .err()
            .map(|error| error.to_string())
    };
    assert_eq!(check(&world, &|_| {}), None);
    type Change = Box<dyn Fn(&mut AutomationInput)>;
    let cases: Vec<(&str, Change)> = vec![
        ("title", Box::new(|d| d.title = "  ".into())),
        ("too long", Box::new(|d| d.title = "x".repeat(121))),
        ("prompt", Box::new(|d| d.prompt = "\n".into())),
        (
            "Project not found",
            Box::new(|d| d.project_id = "nope".into()),
        ),
        ("Unknown agent", Box::new(|d| d.agent = "llama".into())),
        ("model", Box::new(|d| d.model = "a b".into())),
        ("effort", Box::new(|d| d.effort = "warp".into())),
        (
            "account",
            Box::new(|d| d.profile_id = Some("missing".into())),
        ),
        ("Chat or Terminal", Box::new(|d| d.view = "split".into())),
        ("HH:MM", Box::new(|d| d.schedule = daily("25:00"))),
        (
            "one day",
            Box::new(|d| {
                d.schedule = Schedule::Weekdays {
                    at: "09:00".into(),
                    days: vec![],
                }
            }),
        ),
        (
            "1 and 24",
            Box::new(|d| {
                d.schedule = Schedule::Every {
                    hours: 0,
                    from: None,
                }
            }),
        ),
        (
            "1 and 24",
            Box::new(|d| {
                d.schedule = Schedule::Every {
                    hours: 25,
                    from: None,
                }
            }),
        ),
        (
            "HH:MM",
            Box::new(|d| {
                d.schedule = Schedule::Every {
                    hours: 2,
                    from: Some("7am".into()),
                }
            }),
        ),
    ];
    for (needle, change) in &cases {
        let message = check(&world, change.as_ref()).unwrap_or_default();
        assert!(message.contains(needle), "{needle}: {message:?}");
    }
    // An account of the right agent is fine; of the other agent it is not.
    world.workspace.add_profile("Work", Agent::Codex).unwrap();
    let codex = world.workspace.state().profiles[0].id.clone();
    assert!(check(&world, &|d| d.profile_id = Some(codex.clone()))
        .unwrap()
        .contains("account"));
    assert_eq!(
        check(&world, &|d| {
            d.agent = "codex".into();
            d.effort = "medium".into();
            d.profile_id = Some(codex.clone());
        }),
        None
    );
}

#[test]
fn removing_a_project_removes_its_automations() {
    let mut world = world();
    let other_folder = world._root.path().join("other");
    std::fs::create_dir_all(&other_folder).unwrap();
    world.workspace.add_project(&other_folder).unwrap();
    let other = world.workspace.state().projects[1].id.clone();
    let mut store = Store::load(&world.store_file).unwrap();
    store
        .save(input(&world.project), &world.workspace, 1)
        .unwrap();
    store
        .save(input(&world.project), &world.workspace, 2)
        .unwrap();
    store.save(input(&other), &world.workspace, 3).unwrap();
    assert_eq!(store.remove_project(&world.project).unwrap(), 2);
    assert_eq!(store.remove_project(&world.project).unwrap(), 0);
    assert_eq!(store.all().len(), 1);

    // Removed by another build, which never touches this file.
    world.workspace.remove_project(&other).unwrap();
    assert_eq!(store.prune(&world.workspace).unwrap(), 1);
    assert!(Store::load(&world.store_file).unwrap().all().is_empty());

    store
        .save(input(&world.project), &world.workspace, 4)
        .unwrap();
    store.remove_all().unwrap();
    assert!(Store::load(&world.store_file).unwrap().all().is_empty());
}

#[test]
fn runs_finish_skip_and_are_kept_to_twenty() {
    let world = world();
    let mut store = Store::load(&world.store_file).unwrap();
    let id = store
        .save(input(&world.project), &world.workspace, 1)
        .unwrap();
    store.record_start(&id, "s1", None, 10).unwrap();
    assert!(store.get(&id).unwrap().last_run().unwrap().manual);
    assert_eq!(
        store.get(&id).unwrap().last_slot,
        None,
        "Run now is not a slot"
    );
    store
        .record_outcome(
            &id,
            RunStatus::Skipped,
            "still running",
            Some("s1"),
            Some(20),
            21,
        )
        .unwrap();
    let item = store.get(&id).unwrap();
    assert_eq!(item.last_run().unwrap().status, RunStatus::Skipped);
    assert_eq!(item.active_run().unwrap().session_id.as_deref(), Some("s1"));
    assert_eq!(item.last_slot, Some(20));

    let finished = store.finish("s1", RunStatus::Done, "").unwrap().unwrap();
    assert_eq!(finished.id, id);
    assert_eq!(finished.history[1].status, RunStatus::Done);
    assert!(
        store
            .finish("s1", RunStatus::Failed, "x")
            .unwrap()
            .is_none(),
        "once"
    );

    store.record_start(&id, "s2", Some(30), 31).unwrap();
    store.record_start(&id, "s3", Some(40), 41).unwrap();
    assert_eq!(
        store
            .settle_interrupted(&|session| session == "s3")
            .unwrap(),
        1
    );
    let item = store.get(&id).unwrap();
    assert_eq!(item.history[0].status, RunStatus::Started);
    assert_eq!(item.history[1].status, RunStatus::Failed);
    assert!(item.history[1].note.contains("closed"));

    for n in 0..30 {
        store
            .record_outcome(&id, RunStatus::Failed, "no", None, Some(100 + n), 100 + n)
            .unwrap();
    }
    assert_eq!(store.get(&id).unwrap().history.len(), HISTORY);
}
