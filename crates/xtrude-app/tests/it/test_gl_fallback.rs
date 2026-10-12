// SPDX-License-Identifier: GPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 The Xtrude CAD Authors (see AUTHORS)
//! Recovering from a driver that will not give us a context. What is pinned is
//! the DECISION TABLE around the start, with the outcome of the start stubbed:
//! which outcome each situation produces, and above all that the process never
//! restarts itself twice. (wgpu negotiates the surface format, and the
//! viewport picks its own sample count per adapter.)

use xtrude_app::gpu_fallback::{message, on_failure, on_success, Env, Outcome};

fn env(guard: bool, display: bool) -> Env {
    Env { guard, display }
}

#[test]
fn test_a_working_driver_costs_one_probe_and_nothing_else() {
    assert_eq!(on_success(&env(false, true)), Outcome::Ok);
    assert_eq!(message(Outcome::Ok), None);
}

#[test]
fn test_no_context_at_all_restarts_under_xcb() {
    assert_eq!(on_failure("wayland", &env(false, true)), Outcome::Restart);
    // announced before the exec: a silent restart looks like a crash
    assert!(message(Outcome::Restart).is_some_and(|m| m.contains("restarting under X11")));
}

#[test]
fn test_the_restarted_process_does_not_restart_again() {
    assert_eq!(on_failure("x11", &env(true, true)), Outcome::Failed);
    assert_eq!(on_failure("wayland", &env(true, true)), Outcome::Failed);
}

#[test]
fn test_already_on_x11_has_nowhere_to_go() {
    assert_eq!(on_failure("x11", &env(false, true)), Outcome::Failed);
}

#[test]
fn test_wayland_without_xwayland_says_so_instead_of_restarting() {
    assert_eq!(on_failure("wayland", &env(false, false)), Outcome::NoX11);
    assert!(message(Outcome::NoX11).is_some_and(|m| m.contains("no X11 display")));
}

#[test]
fn test_the_restarted_process_reports_that_it_is_the_xcb_one() {
    assert_eq!(on_success(&env(true, true)), Outcome::Xcb);
}
