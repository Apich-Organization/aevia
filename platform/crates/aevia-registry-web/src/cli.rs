//! `/cli` — how the registry and the `aevia` command line fit together.
//!
//! The registry is only half of a package manager; the other half is a
//! toolchain command per action. This page is the map between the two, so a
//! reader who found a package on the site knows what to type, and a reader at
//! a terminal knows which page shows the result.

use aevia_registry_core::Lang;
use aevia_registry_core::vm::Viewer;
use leptos::prelude::AnyView;
use leptos::prelude::*;

use crate::Msg;
use crate::layout;

const TITLE: Msg = Msg::new("command line", "命令行");
const LEDE: Msg = Msg::new(
    "Everything on this site has a command. `aevia` is the toolchain; the registry client \
     `aevia-pkg` signs and uploads, and `aevia` forwards to it.",
    "站点上的每个操作都有对应命令。`aevia` 是工具链；注册中心客户端 `aevia-pkg` 负责签名和上传，\
     `aevia` 会转发给它。",
);
const INSTALL: Msg = Msg::new("Install", "安装");
const INSTALL_BODY: Msg = Msg::new(
    "Build both binaries from the repository and keep them side by side; `aevia` finds \
     `aevia-pkg` beside itself, on `PATH`, or at `$AEVIA_PKG`.",
    "从仓库构建两个可执行文件并放在一起；`aevia` 会在自身旁边、`PATH` 或 `$AEVIA_PKG` 中找到 \
     `aevia-pkg`。",
);
const FIRST_PUBLISH: Msg = Msg::new("Publish a package", "发布一个包");
const FIRST_PUBLISH_BODY: Msg = Msg::new(
    "There are no tokens. `login` binds a key you already own to your account once; after that \
     each publish is a signature. `publish` type-checks and format-checks the package first, so \
     a broken package never reaches the signing prompt.",
    "没有令牌。`login` 把你已有的密钥一次性绑定到账户；此后每次发布都是一次签名。`publish` \
     会先做类型检查和格式检查，所以有问题的包不会走到签名提示这一步。",
);
const USE_PACKAGE: Msg = Msg::new("Use a package", "使用一个包");
const USE_PACKAGE_BODY: Msg = Msg::new(
    "`add` resolves a requirement, writes `aevia.toml` and `aevia.lock`, and vendors the verified \
     sources. After that `aevia run`, `check` and `test` work offline.",
    "`add` 解析版本要求，写入 `aevia.toml` 与 `aevia.lock`，并下载经过校验的源码。之后 \
     `aevia run`、`check`、`test` 均可离线运行。",
);
const MAP: Msg = Msg::new("Site ↔ command", "站点 ↔ 命令");
const COMMAND: Msg = Msg::new("command", "命令");
const WHAT: Msg = Msg::new("what it does", "作用");
const SEE: Msg = Msg::new("on the site", "站点对应页面");

/// One row of the map: a command, what it does, and where the site shows it.
struct Row {
    command: &'static str,
    what: Msg,
    site: &'static str,
}

const ROWS: [Row; 12] = [
    Row {
        command: "aevia login",
        what: Msg::new("bind a signing key to your account in a browser", "在浏览器中把签名密钥绑定到账户"),
        site: "/account",
    },
    Row {
        command: "aevia publish [--dry-run]",
        what: Msg::new("check, format-check, sign and upload", "检查、格式检查、签名并上传"),
        site: "/crates",
    },
    Row {
        command: "aevia yank owner/name@1.2.0 [--undo]",
        what: Msg::new("withdraw or restore a version", "撤回或恢复一个版本"),
        site: "/crates",
    },
    Row {
        command: "aevia search units",
        what: Msg::new("search names, descriptions and keywords", "按名称、描述和关键词搜索"),
        site: "/crates",
    },
    Row {
        command: "aevia info owner/name",
        what: Msg::new("show a package's page in the terminal", "在终端显示包页面"),
        site: "/crates",
    },
    Row {
        command: "aevia add owner/name@^1.2",
        what: Msg::new("add a dependency, re-resolve, vendor", "添加依赖、重新解析并下载"),
        site: "/crates",
    },
    Row {
        command: "aevia update [owner/name]",
        what: Msg::new("move to the newest compatible versions", "升级到最新的兼容版本"),
        site: "/log",
    },
    Row {
        command: "aevia fetch",
        what: Msg::new("download and verify what aevia.lock names", "下载并校验 aevia.lock 中的包"),
        site: "/crates",
    },
    Row {
        command: "aevia tree",
        what: Msg::new("print path and registry dependencies", "打印路径依赖与注册中心依赖"),
        site: "/crates",
    },
    Row {
        command: "aevia verify owner/name@1.2.0",
        what: Msg::new("check a download against its signature", "按签名校验一次下载"),
        site: "/log",
    },
    Row {
        command: "aevia docs owner/name [--open]",
        what: Msg::new("print or open a package's documentation", "打印或打开包的文档"),
        site: "/docs",
    },
    Row {
        command: "aevia doc --open",
        what: Msg::new("build this package's docs locally", "在本地构建当前包的文档"),
        site: "/docs",
    },
];

/// Renders `/cli`.
pub fn cli_page(
    lang: Lang,
    viewer: Option<&Viewer>,
) -> String {
    let rows: Vec<AnyView> = ROWS
        .iter()
        .map(|row| {
            view! {
                <tr>
                    <th scope="row"><code>{row.command}</code></th>
                    <td>{row.what.of(lang)}</td>
                    <td><a href=row.site>{row.site}</a></td>
                </tr>
            }
            .into_any()
        })
        .collect();
    layout::page(
        lang,
        TITLE.of(lang),
        "/cli",
        viewer,
        view! {
            <section class="page-head">
                <h1>{TITLE.of(lang)}</h1>
                <p>{LEDE.of(lang)}</p>
            </section>
            <section class="prose">
                <h2>{INSTALL.of(lang)}</h2>
                <p>{INSTALL_BODY.of(lang)}</p>
                <pre><code>"cargo build --release -p aevia-cli\ncargo build --release --manifest-path platform/Cargo.toml -p aevia-pkg"</code></pre>
                <h2>{FIRST_PUBLISH.of(lang)}</h2>
                <p>{FIRST_PUBLISH_BODY.of(lang)}</p>
                <pre><code>"aevia login\naevia publish --dry-run\naevia publish"</code></pre>
                <h2>{USE_PACKAGE.of(lang)}</h2>
                <p>{USE_PACKAGE_BODY.of(lang)}</p>
                <pre><code>"aevia add apich/units@^1.2\naevia run"</code></pre>
                <h2>{MAP.of(lang)}</h2>
                <table>
                    <thead>
                        <tr>
                            <th scope="col">{COMMAND.of(lang)}</th>
                            <th scope="col">{WHAT.of(lang)}</th>
                            <th scope="col">{SEE.of(lang)}</th>
                        </tr>
                    </thead>
                    <tbody>{rows}</tbody>
                </table>
            </section>
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_guide_names_the_commands_in_both_languages() {
        let en = cli_page(Lang::En, None);
        assert!(en.contains("aevia publish [--dry-run]"));
        assert!(en.contains("aevia docs owner/name [--open]"));
        assert!(en.contains("aria-current=\"page\""), "the nav marks /cli");
        let zh = cli_page(Lang::Zh, None);
        assert!(zh.contains("站点 ↔ 命令"));
        assert!(zh.contains("aevia publish [--dry-run]"), "commands are not translated");
    }
}
