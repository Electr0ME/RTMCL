# Application overview and API review status

RTMCL is an independent Rust launcher prototype with a CLI and a planned terminal
UI. It manages isolated Minecraft: Java Edition instances, Vanilla/Fabric
installation, Java selection, download sources, and game process supervision.

The code and documentation are published to make the project's implementation
and current limitations reviewable. Publication is not a claim of Microsoft or
Mojang approval.

## Authentication

The Microsoft path uses the project's own Microsoft Entra application Client ID
and Device Code flow. It then exchanges tokens through Xbox Live, XSTS, and
Minecraft Services before fetching a player profile. Secrets are excluded from
the repository, and authentication requests do not enter the download mirror
router.

The real Windows check completed Microsoft authorization, Xbox Live, and XSTS;
Minecraft Services token exchange returned HTTP 403. The application owner has
confirmed that no Minecraft API access application has been submitted yet.
Minecraft token/profile retrieval, real system credential persistence and refresh,
and an authenticated game session have not passed acceptance.

## Current authentication boundary

The former development-only local login has been removed from the application:
there is no local-account creation API, offline UUID generator, or local-player launch
option. Launch requires a Microsoft account ID and successful online refresh
through Minecraft Services and player-profile retrieval. Authentication failures
stop launch. Historical local account records are ignored and cannot be selected.

Tests use synthetic Microsoft profiles to verify credential handling; those
fixtures are private to test builds and cannot create a playable CLI session.
The fixed upstream library retains its original generic providers, but RTMCL's
application launch path does not call its offline provider or offline profile
constructor.

The [official API review page](https://help.minecraft.net/hc/en-us/articles/16254801392141)
directs new integrations to the [AppID Review form](https://aka.ms/mce-reviewappid).
That form requires authentication and game ownership checks before gameplay and
does not approve applications that bypass those checks. Removing the local test
path is an implementation change, not proof of approval or completed ownership
acceptance. API approval and real authenticated launch, ownership, credential
persistence, and refresh acceptance remain outstanding.

## Scope and evidence

Earlier game samples were Vanilla 1.21.1 and Fabric Loader 0.16.10 using the now
removed test login. Those screenshots demonstrate historical install/rendering
checks, not the current Microsoft-only session. Evidence and remaining work are in the
[P0 validation record](p0-validation.zh-CN.md). Current local acceptance targets
Windows; WSL / WSLg checks are temporarily suspended.
