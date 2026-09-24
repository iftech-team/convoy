//! Specifications and tasks.
//!
//! The specification-driven half of Convoy: a spec is revised and approved,
//! tasks reference the revision they were written against, and a task session
//! is prepared from both. Nothing here accepts work on the user's behalf —
//! acceptance stays an explicit step, and a specification that moves on sends
//! its completed tasks back for changes.

use adw::prelude::*;
use convoy_core::model::{Agent, PublishMode, Spec, Task, TaskStatus};
use convoy_core::planning::{markdown, SpecInput, TaskInput};
use std::rc::Rc;

use crate::state::App;
use crate::{queue_ui, window};

pub fn open(app: &Rc<App>) {
    let Some(project) = app.selected_project() else {
        app.error("Select a project first.");
        return;
    };
    let navigation = adw::NavigationView::new();
    let dialog = adw::Dialog::builder()
        .title("Specs & tasks")
        .content_width(760)
        .content_height(680)
        .child(&navigation)
        .build();
    navigation.push(&overview(app, &dialog, &navigation, &project.id));
    dialog.present(Some(&app.window));
}

fn overview(
    app: &Rc<App>,
    dialog: &adw::Dialog,
    navigation: &adw::NavigationView,
    project_id: &str,
) -> adw::NavigationPage {
    let page = adw::PreferencesPage::new();

    let specs = adw::PreferencesGroup::builder()
        .title("Specifications")
        .description("A revision is approved before tasks can be prepared from it.")
        .build();
    let workspace = app.workspace.borrow();
    let project_specs: Vec<Spec> = workspace
        .state()
        .specs
        .iter()
        .filter(|spec| spec.project_id == project_id)
        .cloned()
        .collect();
    let project_tasks: Vec<Task> = workspace
        .state()
        .tasks
        .iter()
        .filter(|task| task.project_id == project_id)
        .cloned()
        .collect();
    drop(workspace);

    if project_specs.is_empty() {
        specs.add(
            &adw::ActionRow::builder()
                .title("No specifications yet")
                .subtitle("A task can run without one.")
                .build(),
        );
    }
    for spec in &project_specs {
        let row = adw::ActionRow::builder()
            .title(&spec.title)
            .subtitle(format!(
                "Revision {} · {}",
                spec.revision,
                if spec.approved() { "approved" } else { "draft" }
            ))
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        row.connect_activated({
            let app = app.clone();
            let navigation = navigation.clone();
            let dialog = dialog.clone();
            let spec = spec.clone();
            let project_id = project_id.to_string();
            move |_| {
                navigation.push(&spec_page(
                    &app,
                    &dialog,
                    &navigation,
                    &project_id,
                    Some(spec.clone()),
                ))
            }
        });
        specs.add(&row);
    }
    let new_spec = gtk::Button::with_label("New specification");
    new_spec.connect_clicked({
        let app = app.clone();
        let navigation = navigation.clone();
        let dialog = dialog.clone();
        let project_id = project_id.to_string();
        move |_| navigation.push(&spec_page(&app, &dialog, &navigation, &project_id, None))
    });
    specs.set_header_suffix(Some(&new_spec));

    let tasks = adw::PreferencesGroup::builder()
        .title("Tasks")
        .description("A clean exit moves a task to review. Accepting it is a separate step.")
        .build();
    if project_tasks.is_empty() {
        tasks.add(
            &adw::ActionRow::builder()
                .title("No tasks yet")
                .subtitle("Add one to queue work for an agent.")
                .build(),
        );
    }
    for task in &project_tasks {
        let row = adw::ActionRow::builder()
            .title(&task.title)
            .subtitle(format!(
                "{} · {}{}",
                task.status.as_str(),
                match task.mode {
                    PublishMode::None => "no publishing",
                    PublishMode::Pr => "pull request",
                    PublishMode::Push => "push",
                },
                task.last_error
                    .as_deref()
                    .map(|error| format!(" · {error}"))
                    .unwrap_or_default()
            ))
            .activatable(true)
            .build();
        row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        row.connect_activated({
            let app = app.clone();
            let navigation = navigation.clone();
            let dialog = dialog.clone();
            let task = task.clone();
            let project_id = project_id.to_string();
            let specs = project_specs.clone();
            move |_| {
                navigation.push(&task_page(
                    &app,
                    &dialog,
                    &navigation,
                    &project_id,
                    &specs,
                    Some(task.clone()),
                ))
            }
        });
        tasks.add(&row);
    }
    let new_task = gtk::Button::with_label("New task");
    new_task.connect_clicked({
        let app = app.clone();
        let navigation = navigation.clone();
        let dialog = dialog.clone();
        let project_id = project_id.to_string();
        let specs = project_specs.clone();
        move |_| {
            navigation.push(&task_page(
                &app,
                &dialog,
                &navigation,
                &project_id,
                &specs,
                None,
            ))
        }
    });
    tasks.set_header_suffix(Some(&new_task));

    let queue = adw::PreferencesGroup::builder()
        .title("Queue")
        .description(
            "Runs queued tasks one at a time. A failure pauses it, and a restart never resumes it.",
        )
        .build();
    let queued = project_tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Queued)
        .count();
    let running = queue_ui::is_running(app, project_id);
    let toggle = gtk::Button::with_label(if running { "Pause queue" } else { "Run queue" });
    if !running {
        toggle.add_css_class("suggested-action");
    }
    toggle.set_sensitive(running || queued > 0);
    toggle.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        let project_id = project_id.to_string();
        move |_| {
            if queue_ui::is_running(&app, &project_id) {
                queue_ui::stop(&app, &project_id);
            } else {
                queue_ui::confirm_and_start(&app, &project_id);
            }
            dialog.close();
        }
    });
    let queue_row = adw::ActionRow::builder()
        .title(format!("{queued} queued"))
        .subtitle(if running { "Running" } else { "Paused" })
        .build();
    queue_row.add_suffix(&toggle);
    queue.add(&queue_row);

    page.add(&specs);
    page.add(&tasks);
    page.add(&queue);

    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&adw::HeaderBar::new());
    toolbar.set_content(Some(&page));
    adw::NavigationPage::builder()
        .title("Specs & tasks")
        .child(&toolbar)
        .build()
}

fn field(title: &str, value: &str, lines: i32) -> (adw::PreferencesGroup, gtk::TextView) {
    let view = gtk::TextView::builder()
        .wrap_mode(gtk::WrapMode::WordChar)
        .top_margin(6)
        .bottom_margin(6)
        .left_margin(6)
        .right_margin(6)
        .build();
    view.buffer().set_text(value);
    let frame = gtk::ScrolledWindow::builder()
        .child(&view)
        .height_request(lines * 24)
        .build();
    frame.add_css_class("card");
    let group = adw::PreferencesGroup::builder().title(title).build();
    group.add(&frame);
    (group, view)
}

fn read(view: &gtk::TextView) -> String {
    let buffer = view.buffer();
    buffer
        .text(&buffer.start_iter(), &buffer.end_iter(), false)
        .to_string()
}

fn spec_page(
    app: &Rc<App>,
    dialog: &adw::Dialog,
    navigation: &adw::NavigationView,
    project_id: &str,
    spec: Option<Spec>,
) -> adw::NavigationPage {
    let existing = spec.clone();
    let title = adw::EntryRow::builder().title("Title").build();
    title.set_text(spec.as_ref().map(|spec| spec.title.as_str()).unwrap_or(""));
    let header = adw::PreferencesGroup::new();
    header.add(&title);

    let get = |pick: fn(&Spec) -> &String| -> String {
        spec.as_ref()
            .map(|spec| pick(spec).clone())
            .unwrap_or_default()
    };
    let (problem_group, problem) = field("Problem", &get(|spec| &spec.problem), 4);
    let (requirements_group, requirements) =
        field("Requirements", &get(|spec| &spec.requirements), 5);
    let (acceptance_group, acceptance) = field("Acceptance", &get(|spec| &spec.acceptance), 4);
    let (constraints_group, constraints) = field("Constraints", &get(|spec| &spec.constraints), 3);
    let (plan_group, plan) = field("Plan", &get(|spec| &spec.plan), 5);

    let page = adw::PreferencesPage::new();
    page.add(&header);
    page.add(&problem_group);
    page.add(&requirements_group);
    page.add(&acceptance_group);
    page.add(&constraints_group);
    page.add(&plan_group);

    let toolbar = adw::ToolbarView::new();
    let bar = adw::HeaderBar::new();
    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    bar.pack_end(&save);
    if let Some(spec) = &existing {
        let approve = gtk::Button::with_label(if spec.approved() {
            "Approved"
        } else {
            "Approve revision"
        });
        approve.set_sensitive(!spec.approved());
        approve.connect_clicked({
            let app = app.clone();
            let dialog = dialog.clone();
            let id = spec.id.clone();
            let revision = spec.revision;
            move |_| {
                let outcome = {
                    let mut workspace = app.workspace.borrow_mut();
                    workspace.approve_spec(&id, revision).map(|_| ())
                };
                match outcome {
                    Ok(()) => {
                        dialog.close();
                    }
                    Err(error) => app.error(error),
                }
            }
        });
        bar.pack_end(&approve);

        let export = gtk::Button::from_icon_name("document-save-symbolic");
        export.set_tooltip_text(Some("Export as Markdown"));
        export.connect_clicked({
            let app = app.clone();
            let spec = spec.clone();
            move |_| export_spec(&app, &spec)
        });
        bar.pack_start(&export);
        bar.set_title_widget(Some(&adw::WindowTitle::new(
            &spec.title,
            &format!(
                "Revision {} · {}",
                spec.revision,
                if spec.approved() { "approved" } else { "draft" }
            ),
        )));
    }
    toolbar.add_top_bar(&bar);
    toolbar.set_content(Some(&page));

    save.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        let project_id = project_id.to_string();
        let existing = existing.clone();
        move |_| {
            let input = SpecInput {
                id: existing.as_ref().map(|spec| spec.id.clone()),
                project_id: project_id.clone(),
                title: title.text().to_string(),
                problem: read(&problem),
                requirements: read(&requirements),
                acceptance: read(&acceptance),
                constraints: read(&constraints),
                plan: read(&plan),
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.save_spec(input).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    app.sync();
                    dialog.close();
                }
                Err(error) => app.error(error),
            }
        }
    });

    let _ = navigation;
    adw::NavigationPage::builder()
        .title(if existing.is_some() {
            "Specification"
        } else {
            "New specification"
        })
        .child(&toolbar)
        .build()
}

fn export_spec(app: &Rc<App>, spec: &Spec) {
    let content = markdown(spec, &app.workspace.borrow().state().tasks);
    let dialog = gtk::FileDialog::builder()
        .title("Export specification")
        .initial_name(format!(
            "{}.md",
            spec.title
                .chars()
                .map(|character| if character.is_ascii_alphanumeric() {
                    character
                } else {
                    '-'
                })
                .collect::<String>()
                .trim_matches('-')
        ))
        .build();
    dialog.save(Some(&app.window), gtk::gio::Cancellable::NONE, {
        let app = app.clone();
        move |result| {
            let Ok(file) = result else { return };
            let Some(path) = file.path() else { return };
            match std::fs::write(&path, &content) {
                Ok(()) => app.toasts.add_toast(
                    adw::Toast::builder()
                        .title(format!("Exported to {}", path.display()))
                        .build(),
                ),
                Err(error) => app.error(error.to_string()),
            }
        }
    });
}

fn task_page(
    app: &Rc<App>,
    dialog: &adw::Dialog,
    navigation: &adw::NavigationView,
    project_id: &str,
    specs: &[Spec],
    task: Option<Task>,
) -> adw::NavigationPage {
    let existing = task.clone();
    let title = adw::EntryRow::builder().title("Title").build();
    title.set_text(task.as_ref().map(|task| task.title.as_str()).unwrap_or(""));

    let agents = gtk::StringList::new(&["Claude Code", "Codex"]);
    let agent = adw::ComboRow::builder()
        .title("Agent")
        .model(&agents)
        .build();
    agent.set_selected(match task.as_ref().map(|task| task.agent) {
        Some(Agent::Codex) => 1,
        _ => 0,
    });

    let modes = gtk::StringList::new(&["Do not publish", "Create a pull request", "Push"]);
    let mode = adw::ComboRow::builder()
        .title("Publishing")
        .subtitle("No publishing is the default.")
        .model(&modes)
        .build();
    mode.set_selected(match task.as_ref().map(|task| task.mode) {
        Some(PublishMode::Pr) => 1,
        Some(PublishMode::Push) => 2,
        _ => 0,
    });

    let auto_review = adw::SwitchRow::builder()
        .title("Hand to the other agent when it finishes")
        .active(task.as_ref().is_some_and(|task| task.auto_review))
        .build();

    let mut spec_names: Vec<String> = vec!["None".to_string()];
    spec_names.extend(specs.iter().map(|spec| spec.title.clone()));
    let spec_model =
        gtk::StringList::new(&spec_names.iter().map(String::as_str).collect::<Vec<_>>());
    let spec_row = adw::ComboRow::builder()
        .title("Specification")
        .model(&spec_model)
        .build();
    if let Some(index) = task
        .as_ref()
        .and_then(|task| task.spec_id.clone())
        .and_then(|id| specs.iter().position(|spec| spec.id == id))
    {
        spec_row.set_selected(index as u32 + 1);
    }
    // The specification a task belongs to decides how it is reviewed, so it is
    // fixed once the task exists.
    spec_row.set_sensitive(existing.is_none());

    let settings = adw::PreferencesGroup::new();
    settings.add(&title);
    settings.add(&agent);
    settings.add(&spec_row);
    settings.add(&mode);
    settings.add(&auto_review);

    let (details_group, details) = field(
        "Details",
        task.as_ref()
            .map(|task| task.details.as_str())
            .unwrap_or(""),
        5,
    );
    let (findings_group, findings) = field(
        "Findings",
        task.as_ref()
            .map(|task| task.findings.as_str())
            .unwrap_or(""),
        4,
    );

    let page = adw::PreferencesPage::new();
    page.add(&settings);
    page.add(&details_group);
    page.add(&findings_group);

    if let Some(task) = &existing {
        let status = adw::PreferencesGroup::builder().title("Status").build();
        let row = adw::ActionRow::builder()
            .title(task.status.as_str())
            .subtitle(task.last_error.as_deref().unwrap_or(""))
            .build();
        for (label, target) in [
            ("Back to queue", TaskStatus::Queued),
            ("Needs changes", TaskStatus::Changes),
            ("Accept", TaskStatus::Done),
        ] {
            let button = gtk::Button::with_label(label);
            button.set_valign(gtk::Align::Center);
            if target == TaskStatus::Done {
                button.add_css_class("suggested-action");
            }
            button.connect_clicked({
                let app = app.clone();
                let dialog = dialog.clone();
                let id = task.id.clone();
                move |_| {
                    let outcome = {
                        let mut workspace = app.workspace.borrow_mut();
                        workspace.set_task_status(&id, target).map(|_| ())
                    };
                    match outcome {
                        Ok(()) => {
                            app.sync();
                            dialog.close();
                        }
                        Err(error) => app.error(error),
                    }
                }
            });
            row.add_suffix(&button);
        }
        status.add(&row);

        let prepare = gtk::Button::with_label("Prepare session");
        prepare.connect_clicked({
            let app = app.clone();
            let dialog = dialog.clone();
            let id = task.id.clone();
            move |_| {
                let outcome = {
                    let mut workspace = app.workspace.borrow_mut();
                    workspace.prepare_task(&id, None).map(|_| ())
                };
                match outcome {
                    Ok(()) => {
                        window::rebuild_tabs(&app);
                        app.sync();
                        dialog.close();
                    }
                    Err(error) => app.error(error),
                }
            }
        });
        let prepare_row = adw::ActionRow::builder()
            .title("Session")
            .subtitle("Writes the brief from the approved specification and this task.")
            .build();
        prepare_row.add_suffix(&prepare);
        status.add(&prepare_row);
        page.add(&status);
    }

    let save = gtk::Button::with_label("Save");
    save.add_css_class("suggested-action");
    let bar = adw::HeaderBar::new();
    bar.pack_end(&save);
    let toolbar = adw::ToolbarView::new();
    toolbar.add_top_bar(&bar);
    toolbar.set_content(Some(&page));

    let specs = specs.to_vec();
    save.connect_clicked({
        let app = app.clone();
        let dialog = dialog.clone();
        let project_id = project_id.to_string();
        let existing = existing.clone();
        move |_| {
            let input = TaskInput {
                id: existing.as_ref().map(|task| task.id.clone()),
                project_id: project_id.clone(),
                spec_id: match spec_row.selected() {
                    0 => None,
                    index => specs.get(index as usize - 1).map(|spec| spec.id.clone()),
                },
                title: title.text().to_string(),
                details: read(&details),
                findings: read(&findings),
                agent: if agent.selected() == 0 {
                    Agent::Claude
                } else {
                    Agent::Codex
                },
                mode: match mode.selected() {
                    1 => PublishMode::Pr,
                    2 => PublishMode::Push,
                    _ => PublishMode::None,
                },
                auto_review: auto_review.is_active(),
            };
            let outcome = {
                let mut workspace = app.workspace.borrow_mut();
                workspace.save_task(input).map(|_| ())
            };
            match outcome {
                Ok(()) => {
                    app.sync();
                    dialog.close();
                }
                Err(error) => app.error(error),
            }
        }
    });

    let _ = navigation;
    adw::NavigationPage::builder()
        .title(if existing.is_some() {
            "Task"
        } else {
            "New task"
        })
        .child(&toolbar)
        .build()
}
