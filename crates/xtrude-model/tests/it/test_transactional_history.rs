// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! History.execute is transactional.

use crate::common;
use common::*;
use xtrude_model::commands::AddEdgeCommand;
use xtrude_model::history::FnCommand;
use xtrude_model::CmdError;

#[test]
fn test_failed_command_rolls_back_and_stays_off_the_undo_stack() {
    let (mut s, mut h) = scene_history();
    let dir = std::env::temp_dir().join(format!("xtrude-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let log = dir.join("errors-rollback.log");
    let _ = std::fs::remove_file(&log);
    h.error_log = Some(log.clone());
    h.run(&mut s, AddEdgeCommand::new(V(5., 5.), V(9., 5.)));
    assert_eq!(nedges(&s), 1);
    let ok = h.run(
        &mut s,
        FnCommand::new("ExplodesMidway", |sc: &mut xtrude_model::Scene| {
            sc.mesh_mut().add_edge(V(0., 0.), V(1., 0.))?;
            sc.mesh_mut().add_edge(V(1., 0.), V(2., 5.))?;
            Err(CmdError::msg("ValueError: degenerate edge"))
        }),
    );
    assert!(!ok);
    assert_eq!(nedges(&s), 1);
    assert_eq!(h.undo_len(), 1);
    assert!(h.last_error.as_deref().unwrap().contains("degenerate edge"));
    assert!(std::fs::read_to_string(&log)
        .unwrap()
        .contains("ValueError"));
    assert!(h.undo(&mut s));
    assert_eq!(nedges(&s), 0);
    assert!(h.redo(&mut s));
    assert_eq!(nedges(&s), 1);
}

#[test]
fn test_successful_command_clears_last_error() {
    let (mut s, mut h) = scene_history();
    h.last_error = Some("stale".into());
    h.run(&mut s, AddEdgeCommand::new(V(0., 0.), V(1., 1.)));
    assert!(h.last_error.is_none());
}

/// A PANIC mid-command (an engine bug) rolls back too, like any error.
#[test]
fn test_panicking_command_rolls_back() {
    let (mut s, mut h) = scene_history();
    let dir = std::env::temp_dir().join(format!("xtrude-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.error_log = Some(dir.join("errors-panic.log"));
    let ok = h.run(
        &mut s,
        FnCommand::new("Panics", |sc: &mut xtrude_model::Scene| {
            sc.mesh_mut().add_edge(V(0., 0.), V(1., 0.))?;
            panic!("boom");
        }),
    );
    assert!(!ok);
    assert_eq!(nedges(&s), 0);
    assert_eq!(h.undo_len(), 0);
}

/// `commands_run` counts the commands that ran: not the failed ones, not
/// undo or redo, and on past the undo stack's cap.
#[test]
fn test_commands_run_counts_what_ran() {
    let (mut s, mut h) = scene_history();
    let dir = std::env::temp_dir().join(format!("xtrude-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    h.error_log = Some(dir.join("errors-count.log"));
    h.max_steps = 2;
    assert_eq!(h.commands_run(), 0);
    for i in 0..3 {
        let x = f64::from(i) * 2.;
        assert!(h.run(&mut s, AddEdgeCommand::new(V(x, 0.), V(x + 1., 0.))));
    }
    assert_eq!(h.undo_len(), 2, "the stack is capped");
    assert_eq!(h.commands_run(), 3, "the count is not");
    let failed = FnCommand::new("Fails", |_: &mut xtrude_model::Scene| {
        Err(CmdError::msg("refused"))
    });
    assert!(!h.run(&mut s, failed));
    assert_eq!(h.commands_run(), 3);
    assert!(h.undo(&mut s));
    assert!(h.redo(&mut s));
    assert_eq!(h.commands_run(), 3);
}
