//! The stylesheet and the progressive-enhancement script.
//!
//! Both are constants rather than files on disk because the server serves
//! exactly two static assets and neither changes between builds: keeping them
//! in the crate means `cargo test` covers the markup they target, and the
//! deployment is a single binary with no asset directory to mount or forget.
//!
//! The CSS is hand-written and framework-free on purpose. A registry page is a
//! document — headings, tables, a code block — and the design work is in the
//! spacing and the rules, not in a build step.

/// The whole stylesheet, served as `/static/site.css`.
///
/// Light is the default; the dark block only overrides the custom properties,
/// so every rule below is written once against `var(--…)` and both themes stay
/// in step. The palette is deliberately narrow: one accent, one rule colour,
/// one muted tone. Anything that needs a second accent is a design bug.
pub const SITE_CSS: &str = r#":root {
  --bg: #ffffff;
  --bg-soft: #f5f7f6;
  --fg: #111716;
  --muted: #566663;
  --line: #d7dedb;
  --accent: #0f766e;
  --accent-fg: #ffffff;
  --code-bg: #f2f5f4;
  --warn-fg: #8a4b06;
  --warn-bg: #fdf3dd;
  --max-width: 1100px;
  --mono: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
  --sans: -apple-system, BlinkMacSystemFont, Segoe UI, Roboto, Helvetica, Arial, sans-serif;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #0b100f;
    --bg-soft: #121917;
    --fg: #e4ece9;
    --muted: #93a4a0;
    --line: #232e2b;
    --accent: #2dd4bf;
    --accent-fg: #052a25;
    --code-bg: #111816;
    --warn-fg: #f6c76a;
    --warn-bg: #241d10;
  }
}

*,
*::before,
*::after {
  box-sizing: border-box;
}

html {
  color-scheme: light dark;
}

body {
  margin: 0;
  background: var(--bg);
  color: var(--fg);
  font-family: var(--sans);
  font-size: 16px;
  line-height: 1.6;
  -webkit-font-smoothing: antialiased;
}

a {
  color: var(--accent);
  text-decoration: none;
}

a:hover {
  text-decoration: underline;
}

:focus-visible {
  outline: 2px solid var(--accent);
  outline-offset: 2px;
}

h1,
h2,
h3,
h4 {
  line-height: 1.25;
  letter-spacing: -0.01em;
  margin: 0 0 0.5em;
}

h1 {
  font-size: 2.2rem;
  /* Keeps a two-line Chinese heading from leaving one character alone on the
     second line, which is the most visible way a translated page looks wrong. */
  text-wrap: balance;
}

h2 {
  font-size: 1.25rem;
  margin-top: 2rem;
}

h3 {
  font-size: 1rem;
}

p {
  margin: 0 0 1em;
  max-width: 72ch;
}

ul,
ol {
  padding-left: 1.25rem;
}

code,
pre,
kbd,
samp {
  font-family: var(--mono);
  font-size: 0.875em;
}

pre {
  background: var(--code-bg);
  border: 1px solid var(--line);
  padding: 16px 18px;
  overflow-x: auto;
  line-height: 1.55;
  margin: 0 0 1.25rem;
}

pre code {
  font-size: 0.8125rem;
}

.muted {
  color: var(--muted);
}

.small {
  font-size: 0.8125rem;
}

.skip {
  position: absolute;
  left: -9999px;
  top: 0;
  background: var(--bg);
  padding: 8px 12px;
  z-index: 10;
}

.skip:focus {
  left: 8px;
}

/* Chrome */

.topbar {
  display: flex;
  align-items: center;
  gap: 32px;
  max-width: var(--max-width);
  margin: 0 auto;
  padding: 16px 24px;
  border-bottom: 1px solid var(--line);
}

.wordmark {
  font-family: var(--mono);
  font-size: 1.125rem;
  font-weight: 600;
  letter-spacing: -0.02em;
  color: var(--fg);
}

.wordmark:hover {
  text-decoration: none;
  color: var(--accent);
}

.topnav {
  display: flex;
  gap: 20px;
  font-size: 0.875rem;
}

.topnav a {
  color: var(--muted);
}

.topnav a:hover,
.topnav a[aria-current="page"] {
  color: var(--fg);
  text-decoration: none;
}

.topbar-right {
  margin-left: auto;
  display: flex;
  align-items: center;
  gap: 12px;
  font-size: 0.875rem;
}

.who {
  display: flex;
  align-items: center;
  gap: 10px;
}

.who-handle {
  font-family: var(--mono);
  font-size: 0.8125rem;
}

.avatar {
  width: 20px;
  height: 20px;
  border: 1px solid var(--line);
  vertical-align: middle;
  object-fit: cover;
  background: var(--bg-soft);
}

.avatar-fallback {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  font-family: var(--mono);
  font-size: 0.6875rem;
  text-transform: uppercase;
  color: var(--muted);
}

main {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: 40px 24px 72px;
}

.footer {
  border-top: 1px solid var(--line);
  font-size: 0.8125rem;
  color: var(--muted);
}

.footer-inner {
  max-width: var(--max-width);
  margin: 0 auto;
  padding: 20px 24px 40px;
  display: flex;
  flex-wrap: wrap;
  gap: 8px 32px;
  align-items: baseline;
}

.footer p {
  margin: 0;
}

.footer nav {
  margin-left: auto;
  display: flex;
  gap: 18px;
}

/* Controls */

.btn {
  display: inline-block;
  padding: 8px 14px;
  border: 1px solid transparent;
  background: var(--accent);
  color: var(--accent-fg);
  font: inherit;
  font-size: 0.875rem;
  cursor: pointer;
}

.btn:hover {
  text-decoration: none;
  filter: brightness(1.08);
}

.btn-ghost {
  background: transparent;
  color: var(--fg);
  border-color: var(--line);
}

.btn-ghost:hover {
  border-color: var(--fg);
}

.btn-warn {
  color: var(--warn-fg);
  border-color: var(--warn-fg);
}

.actions {
  display: flex;
  flex-wrap: wrap;
  gap: 10px;
  align-items: center;
  margin: 18px 0 0;
}

.inline {
  display: inline;
  margin: 0;
}

.linkish {
  background: none;
  border: 0;
  padding: 0;
  font: inherit;
  color: var(--muted);
  cursor: pointer;
}

.linkish:hover {
  color: var(--fg);
  text-decoration: underline;
}

/* Install line */

.install {
  display: flex;
  align-items: stretch;
  gap: 0;
  border: 1px solid var(--line);
  background: var(--code-bg);
  margin: 20px 0;
  max-width: 640px;
}

.install code {
  flex: 1;
  padding: 10px 14px;
  overflow-x: auto;
  white-space: nowrap;
  align-self: center;
  font-size: 0.8125rem;
}

.install .copy {
  border: 0;
  border-left: 1px solid var(--line);
  background: transparent;
  color: var(--muted);
  font: inherit;
  font-size: 0.75rem;
  padding: 0 14px;
  cursor: pointer;
  white-space: nowrap;
}

.install .copy:hover {
  color: var(--fg);
}

/* Hero */

.hero {
  padding-bottom: 8px;
}

.hero h1 {
  font-size: 2.6rem;
  max-width: 22ch;
  margin-bottom: 12px;
}

.lede {
  font-size: 1.0625rem;
  color: var(--muted);
  max-width: 62ch;
}

.hero-grid {
  display: grid;
  grid-template-columns: minmax(0, 1.1fr) minmax(0, 1fr);
  gap: 32px;
  align-items: start;
  margin: 32px 0 8px;
}

.stats {
  display: grid;
  grid-template-columns: repeat(2, minmax(0, 1fr));
  gap: 1px;
  background: var(--line);
  border: 1px solid var(--line);
  margin: 0;
}

.stat {
  background: var(--bg);
  padding: 16px 18px;
}

.stat-value {
  display: block;
  font-family: var(--mono);
  font-size: 1.375rem;
  line-height: 1.2;
}

.stat-label {
  display: block;
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.06em;
  color: var(--muted);
  margin-top: 4px;
}

.cols {
  display: grid;
  grid-template-columns: repeat(3, minmax(0, 1fr));
  gap: 32px;
  border-top: 1px solid var(--line);
  margin-top: 48px;
  padding-top: 28px;
}

.cols h2 {
  font-size: 1rem;
  margin-top: 0;
}

.cols p {
  font-size: 0.9375rem;
  color: var(--muted);
}

/* Cards */

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(300px, 1fr));
  gap: 12px;
  margin: 20px 0 8px;
}

.card {
  background: var(--bg);
  border: 1px solid var(--line);
  padding: 16px 18px;
  display: flex;
  flex-direction: column;
  gap: 6px;
}

.card-title {
  font-size: 0.9375rem;
  margin: 0;
  font-family: var(--mono);
  font-weight: 600;
}

.card-title .owner {
  color: var(--muted);
  font-weight: 400;
}

.card-desc {
  margin: 0;
  font-size: 0.875rem;
  color: var(--muted);
  flex: 1;
}

.card-meta {
  margin: 0;
  font-size: 0.75rem;
  color: var(--muted);
  display: flex;
  flex-wrap: wrap;
  gap: 6px;
  align-items: baseline;
}

.card-meta code {
  color: var(--fg);
}

/* Page heads and tabs */

.page-head {
  border-bottom: 1px solid var(--line);
  padding-bottom: 20px;
  margin-bottom: 24px;
}

.page-head h1 {
  font-size: 1.75rem;
  margin-bottom: 6px;
}

.page-head p {
  margin: 0;
  color: var(--muted);
}

.tabs {
  display: flex;
  flex-wrap: wrap;
  gap: 4px 18px;
  font-size: 0.875rem;
  margin: 0 0 20px;
  padding: 0;
  list-style: none;
}

.tabs a {
  color: var(--muted);
  padding-bottom: 3px;
  border-bottom: 2px solid transparent;
}

.tabs a:hover {
  color: var(--fg);
  text-decoration: none;
}

.tabs a[aria-current="page"] {
  color: var(--fg);
  border-bottom-color: var(--accent);
}

/* Search */

.search {
  display: flex;
  gap: 0;
  max-width: 560px;
  margin: 16px 0 20px;
}

.search input[type="search"] {
  flex: 1;
  min-width: 0;
  padding: 8px 12px;
  border: 1px solid var(--line);
  border-right: 0;
  background: var(--bg);
  color: var(--fg);
  font: inherit;
  font-size: 0.9375rem;
}

.search button {
  border: 1px solid var(--line);
  background: var(--bg-soft);
  color: var(--fg);
  font: inherit;
  font-size: 0.875rem;
  padding: 0 16px;
  cursor: pointer;
}

.search button:hover {
  border-color: var(--fg);
}

/* Tables */

table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.875rem;
}

caption {
  text-align: left;
  font-size: 0.8125rem;
  color: var(--muted);
  padding-bottom: 8px;
}

th,
td {
  text-align: left;
  padding: 9px 12px 9px 0;
  border-bottom: 1px solid var(--line);
  vertical-align: top;
}

thead th {
  font-size: 0.6875rem;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--muted);
  font-weight: 600;
  border-bottom: 1px solid var(--line);
  white-space: nowrap;
}

tbody th {
  font-weight: 400;
}

.num {
  text-align: right;
  font-family: var(--mono);
  font-size: 0.8125rem;
  white-space: nowrap;
}

.mono {
  font-family: var(--mono);
  font-size: 0.8125rem;
  word-break: break-all;
}

.badge {
  display: inline-block;
  font-size: 0.6875rem;
  line-height: 1.6;
  padding: 0 6px;
  border: 1px solid var(--line);
  color: var(--muted);
  text-transform: uppercase;
  letter-spacing: 0.05em;
  white-space: nowrap;
}

.badge-yank {
  color: var(--warn-fg);
  border-color: var(--warn-fg);
  background: var(--warn-bg);
}

/* Pagination */

.pager {
  display: flex;
  gap: 16px;
  align-items: baseline;
  font-size: 0.875rem;
  margin-top: 20px;
}

.pager .spacer {
  margin-left: auto;
}

/* Crate page */

.crate-head h1 {
  font-family: var(--mono);
  font-size: 1.6rem;
  letter-spacing: -0.02em;
  word-break: break-word;
}

.crate-head h1 .owner {
  color: var(--muted);
  font-weight: 400;
}

.crate-body {
  display: grid;
  grid-template-columns: minmax(0, 1fr) 280px;
  gap: 48px;
  align-items: start;
  margin-top: 8px;
}

.crate-main {
  min-width: 0;
}

.section-tabs {
  display: flex;
  gap: 20px;
  font-size: 0.8125rem;
  border-bottom: 1px solid var(--line);
  padding-bottom: 10px;
  margin-bottom: 8px;
}

.section-tabs a {
  color: var(--muted);
}

.section-tabs a:hover {
  color: var(--fg);
  text-decoration: none;
}

.side {
  border-left: 1px solid var(--line);
  padding-left: 24px;
  font-size: 0.875rem;
}

.side h2 {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--muted);
  margin: 24px 0 8px;
}

.side h2:first-child {
  margin-top: 0;
}

.meta {
  display: grid;
  grid-template-columns: minmax(0, 1fr);
  gap: 10px;
  margin: 0;
}

.meta div {
  display: grid;
  grid-template-columns: 92px minmax(0, 1fr);
  gap: 8px;
}

.meta dt {
  color: var(--muted);
  font-size: 0.75rem;
  padding-top: 2px;
}

.meta dd {
  margin: 0;
  word-break: break-word;
}

.owners {
  list-style: none;
  padding: 0;
  margin: 0;
  display: grid;
  gap: 8px;
}

.owners li {
  display: flex;
  align-items: center;
  gap: 8px;
}

/* Rendered README and documentation */

.readme,
.doc-body {
  line-height: 1.7;
  overflow-wrap: break-word;
}

.readme h1,
.doc-body h1 {
  font-size: 1.5rem;
  margin-top: 1.6em;
}

.readme h2,
.doc-body h2 {
  font-size: 1.2rem;
  margin-top: 1.6em;
}

.readme h3,
.doc-body h3 {
  font-size: 1rem;
  margin-top: 1.4em;
}

.readme p,
.doc-body p,
.readme li,
.doc-body li {
  max-width: 72ch;
}

.readme pre,
.doc-body pre {
  max-width: 100%;
}

.readme img,
.doc-body img {
  max-width: 100%;
  height: auto;
}

.readme table,
.doc-body table {
  margin: 1em 0;
}

.readme blockquote,
.doc-body blockquote {
  margin: 1em 0;
  padding-left: 16px;
  border-left: 2px solid var(--line);
  color: var(--muted);
}

/* Documentation */

.meter {
  height: 6px;
  background: var(--code-bg);
  border: 1px solid var(--line);
  margin: 6px 0 4px;
}

.meter span {
  display: block;
  height: 100%;
  background: var(--accent);
}

.docs-layout {
  display: grid;
  grid-template-columns: 220px minmax(0, 1fr);
  gap: 40px;
  align-items: start;
}

.docs-side {
  border-right: 1px solid var(--line);
  padding-right: 20px;
  font-size: 0.875rem;
  position: sticky;
  top: 16px;
}

.docs-side h2 {
  font-size: 0.75rem;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--muted);
  margin: 0 0 8px;
}

.docs-side ul {
  list-style: none;
  padding: 0;
  margin: 0 0 20px;
  display: grid;
  gap: 2px;
}

.docs-side a {
  display: block;
  padding: 3px 6px;
  margin-left: -6px;
  color: var(--muted);
  font-family: var(--mono);
  font-size: 0.8125rem;
  overflow-wrap: anywhere;
}

.docs-side a:hover {
  color: var(--fg);
  text-decoration: none;
}

.docs-side a[aria-current="page"] {
  color: var(--fg);
  background: var(--code-bg);
  border-left: 2px solid var(--accent);
}

.docs-main {
  min-width: 0;
}

.doc-head {
  border-bottom: 1px solid var(--line);
  padding-bottom: 12px;
  margin-bottom: 24px;
}

.doc-head h1 {
  font-family: var(--mono);
  font-size: 1.4rem;
  margin-bottom: 4px;
}

.doc-head p {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--muted);
}

/* Account and log */

.viewer-card {
  display: flex;
  align-items: center;
  gap: 16px;
  border: 1px solid var(--line);
  padding: 18px 20px;
  margin-bottom: 32px;
}

.viewer-card .avatar {
  width: 48px;
  height: 48px;
}

.viewer-name {
  font-size: 1.0625rem;
  margin: 0;
}

.viewer-meta {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--muted);
}

.form-grid {
  display: grid;
  gap: 12px;
  max-width: 560px;
  margin: 16px 0 8px;
}

.form-grid label {
  display: grid;
  gap: 4px;
  font-size: 0.8125rem;
  color: var(--muted);
}

.form-grid textarea,
.form-grid input[type="text"] {
  width: 100%;
  padding: 8px 10px;
  border: 1px solid var(--line);
  background: var(--bg);
  color: var(--fg);
  font-family: var(--mono);
  font-size: 0.8125rem;
}

.log-head {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
  gap: 1px;
  background: var(--line);
  border: 1px solid var(--line);
  margin: 20px 0 28px;
}

.log-head div {
  background: var(--bg);
  padding: 14px 16px;
}

.log-head dt {
  font-size: 0.6875rem;
  text-transform: uppercase;
  letter-spacing: 0.07em;
  color: var(--muted);
}

.log-head dd {
  margin: 4px 0 0;
  font-family: var(--mono);
  font-size: 0.8125rem;
  word-break: break-all;
}

/* Error and activation */

.error {
  max-width: 640px;
  padding: 24px 0 40px;
}

.error-status {
  font-family: var(--mono);
  font-size: 3rem;
  line-height: 1;
  color: var(--muted);
  margin: 0 0 8px;
}

.error h1 {
  font-family: var(--mono);
  font-size: 1.5rem;
  margin-bottom: 12px;
}

.activate {
  max-width: 640px;
}

.activate .code {
  font-family: var(--mono);
  font-size: 1.6rem;
  letter-spacing: 0.08em;
  padding: 12px 16px;
  border: 1px solid var(--line);
  background: var(--code-bg);
  display: inline-block;
  margin: 8px 0 20px;
}

/* Responsive */

@media (max-width: 860px) {
  .hero-grid,
  .cols,
  .crate-body,
  .docs-layout {
    grid-template-columns: minmax(0, 1fr);
  }

  .crate-body {
    gap: 32px;
  }

  .side,
  .docs-side {
    border-left: 0;
    border-right: 0;
    border-top: 1px solid var(--line);
    padding: 20px 0 0;
    position: static;
  }

  .hero h1 {
    font-size: 2rem;
  }

  .topbar {
    flex-wrap: wrap;
    gap: 16px;
  }
}

@media (max-width: 620px) {
  /* The top bar has four groups — wordmark, nav, language, identity — and at
     this width they cannot share a row without the last one being clipped. The
     nav gets its own row; the rest stay on the first. */
  .topbar {
    gap: 10px 16px;
  }

  .topnav {
    order: 3;
    flex-basis: 100%;
    gap: 14px;
  }

  .langswitch {
    margin-right: 4px;
  }

  .hero h1 {
    max-width: none;
  }

  pre {
    font-size: 0.75rem;
  }

  /* A three-line example that scrolls sideways on a phone is worse than one
     that wraps: the reader is looking at a snippet, not at a file. */
  .hero-grid pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
}

@media (max-width: 560px) {
  main {
    padding: 28px 16px 56px;
  }

  .topbar,
  .footer-inner {
    padding-left: 16px;
    padding-right: 16px;
  }

  .stats {
    grid-template-columns: minmax(0, 1fr);
  }

  .hero h1 {
    font-size: 1.75rem;
  }
}

@media (prefers-reduced-motion: reduce) {
  * {
    transition: none !important;
    animation: none !important;
  }
}
/* ---------------------------------------------------------------- language */

.langswitch {
  display: flex;
  gap: 2px;
  align-items: center;
  margin-right: 12px;
  font-size: 0.75rem;
}

.langswitch .lang {
  padding: 3px 7px;
  border-radius: 3px;
  color: var(--muted);
  text-decoration: none;
}

.langswitch .lang[aria-current="page"] {
  color: var(--fg);
  background: var(--bg-soft);
  font-weight: 600;
}

.langswitch .lang:hover {
  color: var(--accent);
}

/* --------------------------------------------------------------- spec index */

.spec-intro {
  margin: 32px 0 24px;
}

.spec-table {
  width: 100%;
  border-collapse: collapse;
  font-size: 0.9375rem;
}

.spec-table th {
  text-align: left;
  font-size: 0.75rem;
  letter-spacing: 0.04em;
  text-transform: uppercase;
  color: var(--muted);
  font-weight: 600;
  border-bottom: 1px solid var(--line);
  padding: 8px 10px;
}

.spec-table td {
  border-bottom: 1px solid var(--line);
  padding: 10px;
  vertical-align: top;
}

.spec-table td.num,
.spec-table .num {
  font-family: var(--mono);
  color: var(--muted);
  white-space: nowrap;
  width: 1%;
}

.spec-table a {
  text-decoration: none;
}

.spec-table a:hover {
  text-decoration: underline;
}

.row-sub {
  display: block;
  color: var(--muted);
  font-size: 0.8125rem;
  margin-top: 2px;
}

/* ----------------------------------------------------------- spec chapters */

.spec-shell {
  display: grid;
  grid-template-columns: 232px minmax(0, 1fr);
  gap: 56px;
  align-items: start;
  margin: 32px 0 64px;
}

.spec-nav {
  position: sticky;
  top: 24px;
  font-size: 0.8125rem;
  max-height: calc(100vh - 48px);
  overflow-y: auto;
  padding-right: 8px;
}

.spec-nav ol {
  list-style: none;
  margin: 12px 0;
  padding: 0;
  border-left: 1px solid var(--line);
}

.spec-nav .spec-link {
  display: flex;
  gap: 8px;
  padding: 5px 10px;
  color: var(--muted);
  text-decoration: none;
  border-left: 2px solid transparent;
  margin-left: -1px;
  line-height: 1.35;
}

.spec-nav .spec-link .num {
  font-family: var(--mono);
  opacity: 0.75;
  min-width: 1.6em;
}

.spec-nav .spec-link:hover {
  color: var(--fg);
}

.spec-nav .spec-link.current {
  color: var(--fg);
  border-left-color: var(--accent);
  background: var(--bg-soft);
  font-weight: 600;
}

.spec-nav .spec-home,
.spec-nav .spec-all {
  display: block;
  padding: 4px 0;
  color: var(--muted);
  text-decoration: none;
}

.spec-nav .spec-home:hover,
.spec-nav .spec-all:hover {
  color: var(--accent);
}

.spec-body {
  min-width: 0;
  max-width: 74ch;
}

.spec-head {
  border-bottom: 1px solid var(--line);
  padding-bottom: 16px;
  margin-bottom: 24px;
}

.spec-head h1 {
  margin: 6px 0 4px;
  font-size: 1.9rem;
  line-height: 1.25;
}

.spec-head .eyebrow {
  margin: 0;
  font-family: var(--mono);
  font-size: 0.75rem;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
}

.spec-head .subtitle,
.spec-chapter .subtitle {
  margin: 0 0 10px;
  color: var(--muted);
  font-size: 1rem;
}

.spec-head .meta {
  margin: 8px 0 0;
  font-size: 0.8125rem;
  color: var(--muted);
  display: flex;
  gap: 10px;
  align-items: center;
  flex-wrap: wrap;
}

.abstract {
  border-left: 3px solid var(--accent);
  background: var(--bg-soft);
  padding: 14px 18px;
  margin: 0 0 24px;
  font-size: 0.9375rem;
}

.abstract h2 {
  margin: 0 0 6px;
  font-size: 0.75rem;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
}

.abstract p {
  margin: 0 0 8px;
  line-height: 1.65;
}

.abstract .note {
  margin: 0;
  font-size: 0.8125rem;
  color: var(--muted);
}

.onpage {
  border: 1px solid var(--line);
  padding: 10px 14px;
  margin: 0 0 28px;
  font-size: 0.875rem;
  background: var(--bg-soft);
}

.onpage summary {
  cursor: pointer;
  font-size: 0.75rem;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--muted);
}

.onpage ol {
  margin: 10px 0 4px;
  padding-left: 0;
  list-style: none;
  columns: 2;
  column-gap: 32px;
}

.onpage li {
  margin: 3px 0;
  break-inside: avoid;
}

.onpage a {
  color: var(--muted);
  text-decoration: none;
}

.onpage a:hover {
  color: var(--accent);
}

/* ------------------------------------------------------------ spec content */

.prose {
  font-size: 1rem;
  line-height: 1.72;
}

.prose > h2 {
  margin: 40px 0 12px;
  padding-top: 18px;
  border-top: 1px solid var(--line);
  font-size: 1.25rem;
}

.prose > h2:first-child {
  margin-top: 8px;
  border-top: none;
  padding-top: 0;
}

.prose h3 {
  margin: 28px 0 8px;
  font-size: 1.0625rem;
}

.prose h4 {
  margin: 22px 0 6px;
  font-size: 0.9375rem;
}

.prose p,
.prose li {
  margin: 10px 0;
}

.prose ul,
.prose ol {
  padding-left: 22px;
}

.prose blockquote {
  margin: 16px 0;
  padding: 2px 0 2px 16px;
  border-left: 3px solid var(--line);
  color: var(--muted);
}

.prose table {
  border-collapse: collapse;
  width: 100%;
  margin: 18px 0;
  font-size: 0.875rem;
  display: block;
  overflow-x: auto;
}

.prose th,
.prose td {
  border: 1px solid var(--line);
  padding: 7px 10px;
  text-align: left;
  vertical-align: top;
}

.prose th {
  background: var(--bg-soft);
  font-weight: 600;
}

.prose pre {
  background: var(--code-bg);
  padding: 12px 14px;
  overflow-x: auto;
  font-size: 0.8125rem;
  line-height: 1.55;
  border-left: 2px solid var(--line);
}

.prose code {
  font-family: var(--mono);
  font-size: 0.875em;
}

.prose :not(pre) > code {
  background: var(--code-bg);
  padding: 1px 4px;
}

.prose hr {
  border: none;
  border-top: 1px solid var(--line);
  margin: 32px 0;
}

.chapter-nav {
  display: flex;
  justify-content: space-between;
  gap: 16px;
  margin-top: 48px;
  padding-top: 20px;
  border-top: 1px solid var(--line);
}

.chapter-link {
  display: flex;
  flex-direction: column;
  gap: 3px;
  text-decoration: none;
  max-width: 46%;
}

.chapter-link .direction {
  font-size: 0.75rem;
  letter-spacing: 0.05em;
  text-transform: uppercase;
  color: var(--muted);
}

.chapter-link .title {
  color: var(--accent);
}

.chapter-link.next {
  margin-left: auto;
  text-align: right;
}

.spec-chapter {
  margin: 56px 0 0;
  padding-top: 8px;
}

.spec-chapter > h2 {
  font-size: 1.5rem;
  margin: 0 0 4px;
  padding-top: 24px;
  border-top: 1px solid var(--line);
}

.spec-toc {
  border: 1px solid var(--line);
  padding: 14px 20px;
  margin: 0 0 32px;
}

.spec-toc h2 {
  font-size: 0.75rem;
  letter-spacing: 0.06em;
  text-transform: uppercase;
  color: var(--muted);
  margin: 0 0 8px;
}

.spec-toc ol {
  margin: 0;
  padding-left: 0;
  list-style: none;
  columns: 2;
  column-gap: 32px;
  font-size: 0.875rem;
}

.spec-toc .num {
  font-family: var(--mono);
  color: var(--muted);
  margin-right: 8px;
}

.spec-toc a {
  text-decoration: none;
}

.spec-toc a:hover {
  text-decoration: underline;
}

.badge-quiet {
  background: var(--bg-soft);
  color: var(--muted);
  border: 1px solid var(--line);
}

.empty-page {
  margin: 64px 0;
  max-width: 60ch;
}

@media (max-width: 900px) {
  .spec-shell {
    grid-template-columns: minmax(0, 1fr);
    gap: 24px;
  }

  .spec-nav {
    position: static;
    max-height: none;
    border-bottom: 1px solid var(--line);
    padding-bottom: 16px;
  }

  .onpage ol,
  .spec-toc ol {
    columns: 1;
  }
}

/* ------------------------------------------------------------- sign in page */

.signin {
  max-width: 62ch;
  margin: 48px 0 64px;
}

.signin h1 {
  margin-bottom: 8px;
}

.provider-list {
  display: grid;
  gap: 10px;
  margin: 28px 0 20px;
}

.provider {
  display: flex;
  flex-direction: column;
  gap: 3px;
  padding: 14px 18px;
  border: 1px solid var(--line);
  text-decoration: none;
  color: var(--fg);
  background: var(--bg);
}

a.provider:hover {
  border-color: var(--accent);
}

.provider-name {
  font-weight: 600;
  font-size: 0.9375rem;
}

.provider-note {
  font-size: 0.8125rem;
  color: var(--muted);
}

/* A provider this deployment has not configured: still listed, so an operator
   can see what is missing, but visibly inert. */
.provider-off {
  background: var(--bg-soft);
  border-style: dashed;
}

.provider-off .provider-name {
  color: var(--muted);
  font-weight: 500;
}

/* The "read it as one page" call to action under the specification index. */
.more {
  margin: 20px 0 28px;
}

"#;

/// The script served inline at the end of every page.
///
/// It does two things and nothing else: copy an install line to the clipboard,
/// and filter the crate table while the reader types. Both are enhancements —
/// the install line is selectable text, and the search box submits a normal
/// GET form — so a browser with scripting off loses convenience, not function.
pub const APP_JS: &str = r#"(function () {
  "use strict";

  function flash(button, text) {
    var original = button.getAttribute("data-label");
    if (original === null) {
      original = button.textContent;
      button.setAttribute("data-label", original);
    }
    button.textContent = text;
    window.setTimeout(function () {
      button.textContent = original;
    }, 1200);
  }

  document.addEventListener("click", function (event) {
    var node = event.target;
    if (!(node instanceof Element)) {
      return;
    }
    var button = node.closest("[data-copy]");
    if (!button) {
      return;
    }
    var selector = button.getAttribute("data-copy");
    var source = selector ? document.querySelector(selector) : null;
    if (!source) {
      return;
    }
    var text = source.textContent || "";
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(text).then(function () {
        flash(button, "copied");
      }, function () {
        flash(button, "press ctrl+c");
      });
    } else {
      flash(button, "press ctrl+c");
    }
  });

  var input = document.querySelector("[data-filter]");
  var table = input ? document.querySelector(input.getAttribute("data-filter")) : null;
  if (!input || !table) {
    return;
  }

  var rows = Array.prototype.slice.call(table.querySelectorAll("tbody tr[data-name]"));
  var empty = document.getElementById("crate-filter-empty");

  function apply() {
    var query = input.value.trim().toLowerCase();
    var shown = 0;
    rows.forEach(function (row) {
      var haystack = (row.getAttribute("data-name") || "").toLowerCase();
      var hit = query === "" || haystack.indexOf(query) !== -1;
      row.hidden = !hit;
      if (hit) {
        shown += 1;
      }
    });
    if (empty) {
      empty.hidden = shown !== 0 || query === "";
    }
  }

  input.addEventListener("input", apply);
})();
"#;
