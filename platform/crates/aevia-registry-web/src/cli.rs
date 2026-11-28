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
    "Everything on this site has a command. aevia is the toolchain; the registry client \
     aevia-pkg signs and uploads, and aevia forwards to it.",
    "站点上的每个操作都有对应命令。aevia 是工具链；注册中心客户端 aevia-pkg 负责签名和上传，\
     aevia 会转发给它。",
);
const INSTALL: Msg = Msg::new("Install", "安装");
/// Bootstrap prerequisites shared by the landing page and CLI guide.
pub const INSTALL_PREREQUISITES: Msg = Msg::new(
    "Install GitHub CLI and run gh auth login. The installer verifies the SHA-256 checksum and GitHub release attestation.",
    "请先安装 GitHub CLI 并运行 gh auth login。安装程序会校验 SHA-256 校验和及 GitHub 发布证明。",
);
const CHANNELS: Msg = Msg::new(
    "These commands install the latest stable version. To install a daily build, add -s -- --channel nightly after sh on Unix, or -Channel nightly on Windows. Use --version or -Version to pin a published tag, including a beta or a dated nightly.",
    "以下命令安装最新稳定版。在 Unix 上可在 sh 后加 -s -- --channel nightly 安装每日构建，在 Windows 上使用 -Channel nightly。用 --version 或 -Version 可指定已发布的标签，包括测试版或带日期的每日构建。",
);
const TOOLCHAINS: Msg = Msg::new(
    "After installation, use aevia toolchain --help to install, switch or update toolchains.",
    "安装后可用 aevia toolchain --help 查看安装、切换或更新工具链的命令。",
);
const FIRST_PUBLISH: Msg = Msg::new("Publish a package", "发布一个包");
const FIRST_PUBLISH_BODY: Msg = Msg::new(
    "There are no tokens. login binds a key you already own to your account once; after that \
     each publish is a signature. OpenPGP (gpg) keys are preferred, and hardware-backed keys \
     (such as a YubiKey or OpenPGP smartcard) are recommended; ssh-ed25519 SSH keys are also \
     supported (with a warning). publish type-checks and format-checks the package first, so a \
     broken package never reaches the signing prompt.",
    "没有令牌。login 把你已有的密钥一次性绑定到账户；此后每次发布都是一次签名。系统优先选择并推荐使用 \
     OpenPGP（gpg）密钥及硬件密钥（如 YubiKey 或 OpenPGP 智能卡）；同时也支持 ssh-ed25519 SSH 密钥（并给出推荐警告）。\
     publish 会先做类型检查和格式检查，所以有问题的包不会走到签名提示这一步。",
);
const USE_PACKAGE: Msg = Msg::new("Use a package", "使用一个包");
const USE_PACKAGE_BODY: Msg = Msg::new(
    "add resolves a requirement, writes aevia.toml and aevia.lock, and vendors the verified \
     sources. After that aevia run, check and test work offline.",
    "add 解析版本要求，写入 aevia.toml 与 aevia.lock，并下载经过校验的源码。之后 \
     aevia run、check、test 均可离线运行。",
);
const MAP: Msg = Msg::new("Site ↔ command", "站点 ↔ 命令");
const COMMAND: Msg = Msg::new("command", "命令");
const WHAT: Msg = Msg::new("what it does", "作用");
const SEE: Msg = Msg::new("on the site", "站点对应页面");

/// Copyable install commands for POSIX shells and PowerShell.
pub fn installer_commands(base_url: &str) -> [String; 2] {
    let base = base_url.trim_end_matches('/');
    let sh_base = base.replace('\'', "'\\''");
    let powershell_base = base.replace('\'', "''");
    [
        format!("curl -fsSL '{sh_base}/install.sh' | sh"),
        format!("& ([scriptblock]::Create((irm '{powershell_base}/install.ps1')))"),
    ]
}

/// One row of the map: a command, what it does, and where the site shows it.
struct Row {
    command: &'static str,
    what: Msg,
    site: &'static str,
}

const ROWS: [Row; 13] = [
    Row {
        command: "aevia login",
        what: Msg::new(
            "bind a signing key to your account in a browser",
            "在浏览器中把签名密钥绑定到账户",
        ),
        site: "/account",
    },
    Row {
        command: "aevia publish [--dry-run]",
        what: Msg::new(
            "check, format-check, sign and upload",
            "检查、格式检查、签名并上传",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia publish --owner laboratory",
        what: Msg::new(
            "publish under an enabled GitHub organisation",
            "以已获授权的 GitHub 组织身份发布",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia yank owner/name@1.2.0 [--undo]",
        what: Msg::new("withdraw or restore a version", "撤回或恢复一个版本"),
        site: "/crates",
    },
    Row {
        command: "aevia search units",
        what: Msg::new(
            "search names, descriptions and keywords",
            "按名称、描述和关键词搜索",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia info owner/name",
        what: Msg::new("show a package's page in the terminal", "在终端显示包页面"),
        site: "/crates",
    },
    Row {
        command: "aevia add owner/name@^1.2",
        what: Msg::new(
            "add a dependency, re-resolve, vendor",
            "添加依赖、重新解析并下载",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia update [owner/name]",
        what: Msg::new(
            "move to the newest compatible versions",
            "升级到最新的兼容版本",
        ),
        site: "/log",
    },
    Row {
        command: "aevia fetch",
        what: Msg::new(
            "download and verify what aevia.lock names",
            "下载并校验 aevia.lock 中的包",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia tree",
        what: Msg::new(
            "print path and registry dependencies",
            "打印路径依赖与注册中心依赖",
        ),
        site: "/crates",
    },
    Row {
        command: "aevia verify owner/name@1.2.0",
        what: Msg::new(
            "check a download against its signature",
            "按签名校验一次下载",
        ),
        site: "/log",
    },
    Row {
        command: "aevia docs owner/name [--open]",
        what: Msg::new(
            "print or open a package's documentation",
            "打印或打开包的文档",
        ),
        site: "/docs",
    },
    Row {
        command: "aevia doc --open",
        what: Msg::new(
            "build this package's docs locally",
            "在本地构建当前包的文档",
        ),
        site: "/docs",
    },
];

/// Renders `/cli`.
#[must_use]
pub fn cli_page(
    lang: Lang,
    base_url: &str,
    viewer: Option<&Viewer>,
) -> String {
    let [unix_install, windows_install] = installer_commands(base_url);
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
                <p>{INSTALL_PREREQUISITES.of(lang)}</p>
                <pre><code>{unix_install}</code></pre>
                <pre><code>{windows_install}</code></pre>
                <p>{CHANNELS.of(lang)}</p>
                <p>{TOOLCHAINS.of(lang)}</p>
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
        let en = cli_page(Lang::En, "https://aevia.apich.org", None);
        assert!(en.contains("aevia publish [--dry-run]"));
        assert!(en.contains("aevia docs owner/name [--open]"));
        assert!(en.contains("aria-current=\"page\""), "the nav marks /cli");
        let zh = cli_page(Lang::Zh, "https://aevia.apich.org", None);
        assert!(zh.contains("站点 ↔ 命令"));
        assert!(zh.contains("GitHub 发布证明"));
        assert!(
            zh.contains("aevia publish [--dry-run]"),
            "commands are not translated"
        );
    }
}
