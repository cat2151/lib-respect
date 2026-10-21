use ratatui::text::Line;

use super::{model::Model, theme as t};

pub(super) fn line(model: &Model) -> Line<'static> {
    let Some(account) = model.account.as_ref().filter(|_| model.authenticated) else {
        return Line::styled(
            "Dependency scan: waiting for authentication",
            t::fg(t::ORANGE),
        );
    };
    let Some(catalog) = model
        .catalog
        .as_ref()
        .filter(|c| c.account.id == account.id)
    else {
        return Line::styled(
            "Dependency scan: waiting for project list",
            t::fg(t::ORANGE),
        );
    };
    let mut total = 0;
    let mut checked = 0;
    let mut failed = 0;
    let mut active = None;
    for repo in catalog.repositories.iter().filter(|r| r.eligible(account)) {
        total += 1;
        if let Some(view) = model.views.get(&repo.id) {
            if view.pending {
                active = Some(repo.full_name.as_str());
            } else if view.checked_revision.as_ref() == Some(&repo.pushed_at) {
                checked += 1;
                failed += usize::from(view.error.is_some());
            }
        }
    }
    let (state, color) = if active.is_some() || checked < total {
        ("checking", t::ORANGE)
    } else if model.catalog_loading {
        ("waiting for project list", t::ORANGE)
    } else if failed > 0 {
        ("complete with errors", t::PINK)
    } else {
        ("complete", t::GREEN)
    };
    Line::from(vec![
        t::span("Dependency scan: ", t::MUTED),
        t::span(format!("{state} {checked}/{total}"), color),
        t::span(
            format!(" | {failed} failed"),
            if failed > 0 { t::PINK } else { t::MUTED },
        ),
        t::span(
            active.map(|name| format!(" | {name}")).unwrap_or_default(),
            t::CYAN,
        ),
    ])
}
