// The integrations catalog: every service Convoy can import from, each with
// the connections saved for it. Shared by the Integrations dialog and the
// Settings page, so a new service is one entry in SERVICES, not a button.

import { button, escape } from "./ui.js";
import { t } from "./i18n.js";

/// In display order; connected services move to the top.
export const SERVICES = [
  { kind: "linear", name: "Linear", mark: "L", tint: "#5E6AD2", about: "Issues by team and project" },
  { kind: "jira", name: "Jira", mark: "J", tint: "#2684FF", about: "Issues from Jira Cloud, Server or Data Center" },
  { kind: "notion", name: "Notion", mark: "N", tint: "#37352F", about: "Pages in a database, as tasks" },
  { kind: "peshbar", name: "Peshbar", mark: "P", tint: "#16A34A", about: "Tasks by workspace and project" },
];

const AUTH = { apiKey: "Token", password: "Login & password", mcp: "Agent MCP server" };

/// What a connection reaches and how: "Login & password · jira.acme.com · me@acme.com".
const summary = (item) =>
  [
    item.auth === "apiKey" ? (item.kind === "linear" ? t("API key") : t("Token")) : t(AUTH[item.auth] ?? item.auth),
    item.site?.replace(/^https?:\/\//, "").replace(/\/$/, ""),
    item.username,
  ]
    .filter(Boolean)
    .join(" · ");

const mark = (service) =>
  `<span class="catalog__mark" style="--mark:${service.tint}" aria-hidden="true">${service.mark}</span>`;

function connectionRow(item) {
  const ready = item.auth === "mcp" || item.has_secret;
  return `
    <div class="catalog__connection">
      <span class="catalog__status${ready ? "" : " catalog__status--off"}"
            title="${escape(ready ? t("Ready to use") : t("No saved credentials"))}"></span>
      <div class="catalog__text">
        <div class="catalog__label">${escape(item.name)}</div>
        <div class="catalog__detail">${escape(summary(item))}</div>
      </div>
      ${button({ icon: "edit", kind: "quiet", title: t("Edit"), data: { "conn-edit": item.id } })}
      ${button({ icon: "trash", kind: "quiet", title: t("Remove"), data: { "conn-remove": item.id } })}
    </div>`;
}

/// The whole catalog for `connections`.
export function catalog(connections = []) {
  const of = (kind) => connections.filter((item) => item.kind === kind);
  const ordered = [...SERVICES].sort((a, b) => Number(of(b.kind).length > 0) - Number(of(a.kind).length > 0));
  return `
    <div class="catalog">
      ${ordered
        .map((service) => {
          const saved = of(service.kind);
          return `
            <section class="catalog__service${saved.length ? " catalog__service--on" : ""}">
              <div class="catalog__head">
                ${mark(service)}
                <div class="catalog__text">
                  <div class="catalog__name">${escape(service.name)}</div>
                  <div class="catalog__detail">${escape(t(service.about))}</div>
                </div>
                ${
                  saved.length
                    ? button({ label: t("Add another"), icon: "plus", kind: "quiet", data: { "conn-add": service.kind } })
                    : button({ label: t("Connect"), data: { "conn-add": service.kind } })
                }
              </div>
              ${saved.length ? `<div class="catalog__connections">${saved.map(connectionRow).join("")}</div>` : ""}
            </section>`;
        })
        .join("")}
    </div>`;
}

/// Where secrets live, said once and quietly.
export const storageNote = () =>
  t("Tokens and passwords stay on this computer in {file}, readable by your user only. MCP connections store nothing.", {
    file: "<code>integrations.json</code>",
  });
