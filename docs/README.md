# Documentation

Start with the [English project page](../README.md) or
[中文使用说明](../README.zh-CN.md).

| Document | Purpose |
| --- | --- |
| [Architecture](architecture.zh-CN.md) | Crate boundaries, storage, downloads, accounts, and process ownership |
| [P0 validation](p0-validation.zh-CN.md) | Actual checks, main-menu evidence, and outstanding acceptance items |
| [Microsoft login](microsoft-login.zh-CN.md) | Application settings, confirmed login result, and API review requirements |
| [Application review status](app-review-status.md) | English overview, Microsoft-only launch boundary, authentication evidence, and remaining acceptance |
| [Roadmap](launcher-plan.zh-CN.md) | Planned scope and delivery phases; goals are not implementation claims |
| [Lighty audit](lighty-release-audit.json) | Checksums and comparison of the nine original release packages |
| [Lighty patches](../vendor/lighty/RTMCL-PATCHES.md) | Fixed upstream revision and local changes |

`evidence/` contains the Windows and historical WSLg main-menu screenshots.
Runtime downloads, personal account data, and raw logs stay outside the Git
publication set under the ignored `.rtmcl/` directory.
