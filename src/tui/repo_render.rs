use super::{repo_list::RepoList, theme as t};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    text::Line,
    widgets::{Block, List, ListItem, ListState, Paragraph, Wrap},
};

fn one_line(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn draw(frame: &mut Frame, list: &RepoList) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(6),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let unknown = list
        .rows
        .iter()
        .filter(|row| row.viewer.starred.is_none())
        .count();
    let visible = list.visible();
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                t::span("lib-respect", t::PINK),
                t::span(" / Dependency repositories ", t::GREEN),
                t::span(
                    if list.descending {
                        "[stars descending]"
                    } else {
                        "[stars ascending]"
                    },
                    t::PURPLE,
                ),
            ]),
            Line::from(vec![
                t::span("Filter: ", t::MUTED),
                t::span(list.filter.label(), t::YELLOW),
                t::span(
                    format!(
                        " | {} / {} repos | {unknown} unknown",
                        visible.len(),
                        list.rows.len()
                    ),
                    t::PURPLE,
                ),
                t::span(
                    if list.loading { " | refreshing..." } else { "" },
                    t::ORANGE,
                ),
            ]),
            Line::from(vec![
                t::span("Account: ", t::MUTED),
                t::span(list.viewer_login.as_deref().unwrap_or("unknown"), t::PURPLE),
                t::span(" | Own repos: ", t::MUTED),
                t::span(
                    if list.include_own {
                        "included"
                    } else {
                        "excluded"
                    },
                    t::YELLOW,
                ),
            ]),
        ]),
        areas[0],
    );
    let items: Vec<_> = visible
        .iter()
        .map(|row| {
            let stars = row
                .stats
                .stars
                .map_or_else(|| "?".into(), |n| n.to_string());
            let warning = if row.viewer.error.is_some() || row.stats.error.is_some() {
                " !"
            } else {
                ""
            };
            let description = row
                .stats
                .description
                .as_deref()
                .map(one_line)
                .unwrap_or_else(|| {
                    if row.stats.details_fetched_at.is_some() {
                        "No description provided".into()
                    } else {
                        "Description not fetched yet".into()
                    }
                });
            let projects = row
                .projects
                .iter()
                .map(|url| url.trim_start_matches("https://github.com/"))
                .collect::<Vec<_>>()
                .join(", ");
            let libraries = row.libraries.iter().cloned().collect::<Vec<_>>().join(", ");
            let show_libraries = row.url.rsplit('/').next() != Some(libraries.as_str());
            ListItem::new(vec![
                Line::from(vec![
                    t::span(row.url.trim_start_matches("https://github.com/"), t::GREEN),
                    t::span(if show_libraries { "  Libraries: " } else { "" }, t::MUTED),
                    t::span(
                        if show_libraries {
                            libraries
                        } else {
                            String::new()
                        },
                        t::PURPLE,
                    ),
                    t::span("  ", t::MUTED),
                    t::span(
                        description,
                        if row.stats.description.is_some() {
                            t::FOREGROUND
                        } else {
                            t::MUTED
                        },
                    ),
                    t::span(format!("  {stars} stars"), t::YELLOW),
                    t::span(warning, t::PINK),
                ]),
                Line::from(vec![
                    t::span(
                        if row.viewer.starred == Some(true) {
                            "★  "
                        } else {
                            ""
                        },
                        t::GREEN,
                    ),
                    t::span("Used by: ", t::MUTED),
                    t::span(projects, t::CYAN),
                ]),
            ])
        })
        .collect();
    let mut state =
        ListState::default().with_selected((!items.is_empty()).then_some(list.selected));
    frame.render_stateful_widget(
        List::new(items)
            .style(t::fg(t::CYAN))
            .block(
                Block::bordered()
                    .title(" GitHub repositories ")
                    .border_style(t::fg(t::BORDER))
                    .title_style(t::fg(t::CYAN)),
            )
            .highlight_symbol("> ")
            .highlight_style(t::selection(true)),
        areas[1],
        &mut state,
    );
    let detail = if let Some(row) = list.current() {
        let projects = row
            .projects
            .iter()
            .map(|url| url.trim_start_matches("https://github.com/"))
            .collect::<Vec<_>>()
            .join(", ");
        let status = if let Some(error) = &row.viewer.error {
            format!("Star check failed (previous state retained if known): {error}")
        } else if let Some(error) = &row.stats.error {
            format!("Metadata refresh failed: {error}")
        } else {
            String::new()
        };
        vec![
            Line::styled(&row.url, t::fg(t::CYAN)),
            Line::from(vec![
                t::span("Used by: ", t::MUTED),
                t::span(projects, t::CYAN),
            ]),
            Line::styled(status, t::fg(t::PINK)),
            Line::styled(&list.notice, t::notice(&list.notice)),
        ]
    } else {
        vec![
            Line::styled("No repositories match this filter.", t::fg(t::ORANGE)),
            Line::styled("Unknown star states are shown under Both.", t::fg(t::MUTED)),
            Line::styled(
                "Browse projects to discover more dependencies.",
                t::fg(t::MUTED),
            ),
            Line::styled(&list.notice, t::notice(&list.notice)),
        ]
    };
    frame.render_widget(Paragraph::new(detail).wrap(Wrap { trim: false }), areas[2]);
    frame.render_widget(
        Paragraph::new(
            "Tab projects j/k Home/End PgUp/Dn(10) s sort f filter o own Enter open q quit",
        )
        .style(t::fg(t::MUTED)),
        areas[3],
    );
}
