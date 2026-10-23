//! The list as text and as JSON: what `demopen` prints where there is no view.

use bt_view::demo::{Containers, State, readings};
use serde_json::json;

fn state(state: State) -> (&'static str, Option<i32>) {
    match state {
        State::Running => ("running", None),
        State::Starting => ("starting", None),
        State::Unhealthy => ("unhealthy", None),
        State::Paused => ("paused", None),
        State::Exited(code) => ("exited", Some(code)),
    }
}

/// The list as a table.
pub fn text() -> String {
    let page = Containers::fixture();
    let mut out = format!(
        "{:<10} {:<8} {:<14} {:>7}  {:>8}  {}\n",
        "NAME", "PROJECT", "STATE", "CPU", "MEMORY", "IMAGE"
    );
    for c in page.containers() {
        let (name, code) = state(c.state);
        let state = match code {
            Some(code) => format!("{name} ({code})"),
            None => name.to_owned(),
        };
        let (cpu, memory) = if code.is_some() {
            (String::from("-"), String::from("-"))
        } else {
            (format!("{:.1}%", c.cpu), c.memory.0.clone())
        };
        out.push_str(&format!(
            "{:<10} {:<8} {:<14} {:>7}  {:>8}  {}\n",
            c.name,
            c.project.unwrap_or("-"),
            state,
            cpu,
            memory,
            c.image
        ));
    }
    out
}

/// `value` to the nearest `1 / scale`, as the number a person would write.
fn rounded(value: f32, scale: f64) -> f64 {
    (f64::from(value) * scale).round() / scale
}

/// The list as JSON, a line for each container.
pub fn json() -> String {
    let page = Containers::fixture();
    let now = readings(0);
    let mut out = String::new();
    for c in page.containers() {
        let (name, code) = state(c.state);
        let reading = now.iter().find(|r| r.name == c.name);
        let line = json!({
            "name": c.name,
            "project": c.project,
            "image": c.image,
            "state": name,
            "exit_code": code,
            "cpu": rounded(reading.map_or(c.cpu, |r| r.cpu), 10.0),
            "memory": reading.map_or(c.memory.0.as_str(), |r| r.memory.as_str()),
            "share": rounded(reading.map_or(c.memory.1, |r| r.share), 1000.0),
            "ports": c.ports,
            "age": c.age,
        });
        out.push_str(&line.to_string());
        out.push('\n');
    }
    out
}
