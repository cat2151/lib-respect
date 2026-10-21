use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Modifier,
    text::Line,
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

use super::{
    model::{Focus, Model},
    theme as t,
};

pub fn draw(frame: &mut Frame, model: &Model) {
    frame.render_widget(Block::default().style(t::base()), frame.area());
    if model.screen == super::repo_list::Screen::Repositories {
        super::repo_render::draw(frame, &model.repositories);
        return;
    }
    let area = frame.area();
    let sections = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(3),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(area);
    let account = model
        .account
        .as_ref()
        .map_or("connecting...", |a| a.login.as_str());
    let auth = if model.authenticated {
        ""
    } else {
        " (previous account / checking auth)"
    };
    let source = if model.cached_catalog {
        "cached"
    } else {
        "live"
    };
    let activity = if model.catalog_loading {
        " / refreshing"
    } else {
        ""
    };
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                t::span("lib-respect", t::PINK).style(t::fg(t::PINK).add_modifier(Modifier::BOLD)),
                t::span("  github.com / ", t::MUTED),
                t::span(account, t::PURPLE),
                t::span(auth, t::ORANGE),
            ]),
            Line::styled(
                format!("Owned public projects | non-archived | non-fork | {source}{activity}"),
                t::fg(t::MUTED),
            ),
            super::scan_status::line(model),
        ]),
        sections[0],
    );
    let panes = Layout::horizontal([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(sections[1]);
    projects(frame, model, panes[0]);
    dependencies(frame, model, panes[1]);
    frame.render_widget(
        Paragraph::new(model.notice.as_str())
            .style(t::notice(&model.notice))
            .wrap(Wrap { trim: false }),
        sections[2],
    );
    let footer = if model.searching {
        format!("Search: {}_  | Enter accept  Esc clear", model.query)
    } else {
        "Tab repos h/l pane j/k Home/End PgUp/Dn(10) / search r/R refresh q quit".into()
    };
    frame.render_widget(
        Paragraph::new(footer).style(t::fg(if model.searching { t::YELLOW } else { t::MUTED })),
        sections[3],
    );
}

fn border(title: String, active: bool) -> Block<'static> {
    Block::bordered()
        .title(title)
        .border_style(t::fg(if active { t::CYAN } else { t::BORDER }))
        .title_style(t::fg(if active { t::CYAN } else { t::MUTED }))
}

fn projects(frame: &mut Frame, model: &Model, area: Rect) {
    let repos = model.filtered();
    let title = format!(
        " Projects {}/{} {} ",
        if repos.is_empty() {
            0
        } else {
            model.selected + 1
        },
        repos.len(),
        if model.query.is_empty() {
            String::new()
        } else {
            format!("/{}", model.query)
        }
    );
    let items: Vec<_> = repos
        .iter()
        .map(|repo| {
            let marker = model.views.get(&repo.id).map_or(" ", |view| {
                if view.pending {
                    "~"
                } else if view.error.is_some() {
                    "!"
                } else if view.checked_revision.as_ref() == Some(&repo.pushed_at) {
                    "✓"
                } else {
                    " "
                }
            });
            ListItem::new(Line::from(vec![
                t::span(
                    marker,
                    match marker {
                        "!" => t::PINK,
                        "~" => t::ORANGE,
                        "✓" => t::GREEN,
                        _ => t::MUTED,
                    },
                ),
                t::span(format!(" {}", repo.name), t::GREEN),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(if repos.is_empty() {
        None
    } else {
        Some(model.selected)
    });
    frame.render_stateful_widget(
        List::new(items)
            .style(t::fg(t::CYAN))
            .block(border(title, model.focus == Focus::Projects))
            .highlight_symbol("> ")
            .highlight_style(t::selection(model.focus == Focus::Projects)),
        area,
        &mut state,
    );
}

fn dependencies(frame: &mut Frame, model: &Model, area: Rect) {
    let name = model
        .current()
        .map_or("Select a project", |r| r.name.as_str());
    let block = border(
        format!(" {name} / declared dependencies "),
        model.focus == Focus::Dependencies,
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(view) = model.view() else {
        frame.render_widget(
            Paragraph::new("Select a project with j/k.\nDependencies appear here automatically.")
                .style(t::fg(t::MUTED)),
            inner,
        );
        return;
    };
    let Some(data) = &view.data else {
        let text = view.error.as_deref().unwrap_or(if view.pending {
            "Loading Cargo / CMake declarations..."
        } else {
            "Waiting for authenticated account..."
        });
        frame.render_widget(
            Paragraph::new(text)
                .style(t::fg(if view.error.is_some() {
                    t::PINK
                } else {
                    t::ORANGE
                }))
                .wrap(Wrap { trim: false }),
            inner,
        );
        return;
    };
    let report = &data.report;
    let areas = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(1),
        Constraint::Length(5),
    ])
    .split(inner);
    let state = if view.pending {
        "cached / updating..."
    } else if view.error.is_some() {
        "cached / update failed"
    } else {
        "cached"
    };
    let mapped = report
        .dependencies
        .iter()
        .filter(|dep| dep.github_url.is_some())
        .count();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                t::span(
                    format!(
                        "{} declarations / {mapped} GitHub URLs / {} manifests | ",
                        report.dependencies.len(),
                        report.manifest_count
                    ),
                    t::PURPLE,
                ),
                t::span(
                    state,
                    if view.error.is_some() {
                        t::PINK
                    } else if view.pending {
                        t::ORANGE
                    } else {
                        t::MUTED
                    },
                ),
            ]),
            Line::styled(
                "Static declarations; includes inactive branches and optional deps",
                t::fg(t::MUTED),
            ),
        ]),
        areas[0],
    );
    if report.manifest_count == 0 {
        frame.render_widget(
            Paragraph::new("No Cargo.toml or CMake files found in this repository.")
                .style(t::fg(t::MUTED)),
            areas[1],
        );
        return;
    }
    let items: Vec<_> = report
        .dependencies
        .iter()
        .map(|dep| {
            let optional = if dep.optional { " ?" } else { "" };
            ListItem::new(vec![
                Line::from(vec![
                    t::span(&dep.name, t::GREEN),
                    t::span(format!("  {}", dep.requirement), t::PURPLE),
                    t::span(format!("  [{}]{optional}", dep.kind), t::ORANGE),
                ]),
                Line::styled(
                    dep.github_url.clone().unwrap_or_else(|| {
                        dep.repository_note
                            .clone()
                            .unwrap_or_else(|| "Repository lookup pending".into())
                    }),
                    t::fg(if dep.github_url.is_some() {
                        t::CYAN
                    } else {
                        t::MUTED
                    }),
                ),
            ])
        })
        .collect();
    let mut selected = ListState::default().with_selected(if items.is_empty() {
        None
    } else {
        Some(model.dependency_selected.min(items.len() - 1))
    });
    frame.render_stateful_widget(
        List::new(items)
            .style(t::fg(t::CYAN))
            .highlight_symbol("> ")
            .highlight_style(t::selection(model.focus == Focus::Dependencies)),
        areas[1],
        &mut selected,
    );
    if let Some(dep) = report.dependencies.get(model.dependency_selected) {
        let target = dep.target.as_deref().unwrap_or("all targets");
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    t::span(&dep.source, t::ORANGE),
                    t::span(format!(" | alias: {} | {target}", dep.alias), t::PURPLE),
                ]),
                Line::styled(
                    dep.github_url
                        .as_deref()
                        .unwrap_or("GitHub URL unavailable"),
                    t::fg(if dep.github_url.is_some() {
                        t::CYAN
                    } else {
                        t::ORANGE
                    }),
                ),
                Line::styled(
                    dep.repository_note
                        .as_deref()
                        .unwrap_or("Repository lookup pending"),
                    t::fg(t::MUTED),
                ),
                Line::styled(&dep.manifest, t::fg(t::MUTED)),
            ])
            .wrap(Wrap { trim: false }),
            areas[2],
        );
    }
}
