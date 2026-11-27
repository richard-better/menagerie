//! The page that commits your checkout's uncommitted changes and pushes
//! them, so a sync can deploy them.
//!
//! It opens with a message made from the file names, and, when the fleet
//! file names a drafting command, replaces it with that command's draft
//! unless you have started typing.

use std::path::PathBuf;

use gpui::{
    Context, Entity, EventEmitter, FontWeight, IntoElement, ParentElement, Render, Styled,
    Subscription, Task, Window, div, prelude::*, px, rgb,
};

use crate::commit::{self, Change};
use crate::theme;
use crate::ui::text_field::{Submit, TextField};
use crate::ui::widgets::text_button;

/// The changes were committed and pushed, as this short hash.
pub struct Committed(pub String);

enum Changes {
    Loading,
    Ready(Vec<Change>),
    Failed(String),
}

enum Draft {
    None,
    Running,
    Done,
    Failed(String),
}

pub struct CommitPage {
    repo: PathBuf,
    branch: String,
    command: Option<String>,
    changes: Changes,
    /// Files whose contents weren't given to the drafting command.
    withheld: Vec<String>,
    field: Entity<TextField>,
    draft: Draft,
    committing: bool,
    error: Option<String>,
    _tasks: Vec<Task<()>>,
    _submit: Subscription,
}

impl EventEmitter<Committed> for CommitPage {}

impl CommitPage {
    pub fn new(
        repo: PathBuf,
        branch: String,
        command: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let field = cx.new(|cx| TextField::new("Commit message", cx));
        window.focus(&gpui::Focusable::focus_handle(field.read(cx), cx));
        let submit = cx.subscribe(&field, |this: &mut Self, _, _: &Submit, cx| this.commit(cx));
        let mut page = Self {
            repo,
            branch,
            command,
            changes: Changes::Loading,
            withheld: Vec::new(),
            field,
            draft: Draft::None,
            committing: false,
            error: None,
            _tasks: Vec::new(),
            _submit: submit,
        };
        page.load(cx);
        page
    }

    /// List the changes, fill in the template, then ask for a draft.
    fn load(&mut self, cx: &mut Context<Self>) {
        let repo = self.repo.clone();
        let command = self.command.clone();
        let task = cx.spawn(async move |this, cx| {
            let listed = cx
                .background_executor()
                .spawn({
                    let repo = repo.clone();
                    async move { commit::changes(&repo) }
                })
                .await;
            let changes = match listed {
                Ok(changes) => changes,
                Err(e) => {
                    let _ = this.update(cx, |page, cx| {
                        page.changes = Changes::Failed(e);
                        cx.notify();
                    });
                    return;
                }
            };
            let empty = changes.is_empty();
            let ok = this.update(cx, |page, cx| {
                let template = commit::template(&changes);
                page.field.update(cx, |f, cx| {
                    if !f.edited() {
                        f.set_text(template, cx)
                    }
                });
                if command.is_some() && !empty {
                    page.draft = Draft::Running;
                }
                page.changes = Changes::Ready(changes.clone());
                cx.notify();
            });
            let (Ok(()), Some(command), false) = (ok, command, empty) else {
                return;
            };
            let drafted = cx
                .background_executor()
                .spawn(async move {
                    let (input, withheld) = commit::draft_input(&repo, &changes)?;
                    let message = commit::draft(&command, &repo, &input, commit::DRAFT_TIMEOUT);
                    Ok::<_, String>((message, withheld))
                })
                .await;
            let _ = this.update(cx, |page, cx| {
                match drafted {
                    Ok((Ok(message), withheld)) => {
                        page.withheld = withheld;
                        page.field.update(cx, |f, cx| {
                            if !f.edited() {
                                f.set_text(message, cx)
                            }
                        });
                        page.draft = Draft::Done;
                    }
                    Ok((Err(e), withheld)) => {
                        page.withheld = withheld;
                        page.draft = Draft::Failed(e);
                    }
                    Err(e) => page.draft = Draft::Failed(e),
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
    }

    fn commit(&mut self, cx: &mut Context<Self>) {
        let Changes::Ready(changes) = &self.changes else {
            return;
        };
        if self.committing || changes.is_empty() {
            return;
        }
        let paths: Vec<String> = changes.iter().map(|c| c.path.clone()).collect();
        let message = self.field.read(cx).text().to_owned();
        let repo = self.repo.clone();
        let branch = self.branch.clone();
        self.committing = true;
        self.error = None;
        cx.notify();
        let task = cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { commit::commit_and_push(&repo, &branch, &paths, &message) })
                .await;
            let _ = this.update(cx, |page, cx| {
                page.committing = false;
                match result {
                    Ok(hash) => cx.emit(Committed(hash)),
                    Err(e) => page.error = Some(e),
                }
                cx.notify();
            });
        });
        self._tasks.push(task);
    }
}

impl Render for CommitPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let red = rgb(0xe5534b);
        let mono = |text: String| {
            div()
                .font_family(theme::MONO)
                .text_size(px(11.5))
                .child(text)
        };
        let list = match &self.changes {
            Changes::Loading => div()
                .text_color(theme::mute())
                .text_size(px(12.))
                .child("Looking for changes…"),
            Changes::Failed(e) => div().text_color(red).text_size(px(12.)).child(e.clone()),
            Changes::Ready(changes) if changes.is_empty() => div()
                .text_color(theme::mute())
                .text_size(px(12.))
                .child("Nothing to commit: the checkout is clean."),
            Changes::Ready(changes) => {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(changes.iter().map(|change| {
                        let (added, removed) = match change.lines {
                            Some((a, r)) => (format!("+{a}"), format!("-{r}")),
                            None => (String::new(), String::new()),
                        };
                        div()
                            .flex()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                mono(change.kind.to_owned())
                                    .w(px(76.))
                                    .flex_none()
                                    .text_color(theme::mute()),
                            )
                            .child(mono(change.path.clone()).flex_1().truncate())
                            .child(mono(added).text_color(theme::green()))
                            .child(mono(removed).text_color(red))
                    }))
            }
        };
        let count = match &self.changes {
            Changes::Ready(c) if !c.is_empty() => format!(
                "Changes · {} file{}",
                c.len(),
                if c.len() == 1 { "" } else { "s" }
            ),
            _ => "Changes".to_owned(),
        };
        let note = match &self.draft {
            Draft::None if self.command.is_none() => Some((
                "Add `commit_message` to the fleet file to have a coding agent draft this."
                    .to_owned(),
                theme::mute(),
            )),
            Draft::None => None,
            Draft::Running => Some(("Drafting with your command…".to_owned(), theme::mute())),
            Draft::Done => Some((
                "Drafted by your command. Edit it as you like.".to_owned(),
                theme::mute(),
            )),
            Draft::Failed(e) => Some((
                format!("{e}. The message is made from the file names."),
                theme::mute(),
            )),
        };
        let ready = matches!(&self.changes, Changes::Ready(c) if !c.is_empty());
        let can_commit = ready && !self.committing && !self.field.read(cx).text().trim().is_empty();
        div()
            .flex()
            .flex_col()
            .px(px(20.))
            .pt(px(18.))
            .pb(px(20.))
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .mb(px(10.))
                    .child(count),
            )
            .child(list)
            .child(
                div()
                    .mt(px(22.))
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .mb(px(10.))
                    .child("Message"),
            )
            .child(self.field.clone())
            .when_some(note, |el, (text, colour)| {
                el.child(
                    div()
                        .mt(px(6.))
                        .text_size(px(11.5))
                        .text_color(colour)
                        .child(text),
                )
            })
            .when(!self.withheld.is_empty(), |el| {
                el.child(
                    div()
                        .mt(px(4.))
                        .text_size(px(11.5))
                        .text_color(theme::mute())
                        .child(format!(
                            "Not shown to the command, as they may hold secrets: {}",
                            self.withheld.join(", ")
                        )),
                )
            })
            .child(
                div()
                    .mt(px(16.))
                    .flex()
                    .items_center()
                    .justify_end()
                    .gap(px(12.))
                    .when_some(self.error.clone(), |el, e| {
                        el.child(div().flex_1().text_size(px(12.)).text_color(red).child(e))
                    })
                    .child(
                        text_button(
                            "commit",
                            if self.committing {
                                "Committing…"
                            } else {
                                "Commit & push"
                            },
                            can_commit,
                        )
                        .when(can_commit, |b| {
                            b.bg(rgb(0x2563eb))
                                .text_color(rgb(0xffffff))
                                .hover(|s| s.bg(rgb(0x3b74f0)))
                                .on_click(cx.listener(|this, _, _, cx| this.commit(cx)))
                        }),
                    ),
            )
    }
}
