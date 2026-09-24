//! Port of `markdown()` from planning.cjs — the specification export and the
//! preamble of every prepared task brief.

use crate::workspace::model::{Spec, Task, TaskStatus};

type Section = (&'static str, fn(&Spec) -> &String);

const SECTIONS: [Section; 5] = [
    ("Problem", |spec| &spec.problem),
    ("Requirements", |spec| &spec.requirements),
    ("Acceptance", |spec| &spec.acceptance),
    ("Constraints", |spec| &spec.constraints),
    ("Plan", |spec| &spec.plan),
];

pub fn markdown(spec: &Spec, tasks: &[Task]) -> String {
    let status = if spec.approved() { "Approved" } else { "Draft" };
    let mut output = format!(
        "# {}\n\nRevision: {} · {}\n\n",
        spec.title, spec.revision, status
    );
    let body: Vec<String> = SECTIONS
        .iter()
        .map(|(heading, field)| format!("## {heading}\n\n{}\n", field(spec)))
        .collect();
    output.push_str(&body.join("\n"));
    output.push_str("\n## Tasks\n\n");
    let listed: Vec<String> = tasks
        .iter()
        .filter(|task| task.spec_id.as_deref() == Some(spec.id.as_str()))
        .map(|task| {
            let mark = if task.status == TaskStatus::Done { "x" } else { " " };
            format!(
                "- [{mark}] {} ({})\n  {}\n  Findings: {}",
                task.title,
                task.status.as_str(),
                task.details,
                task.findings
            )
        })
        .collect();
    output.push_str(&listed.join("\n"));
    output.push('\n');
    output
}
