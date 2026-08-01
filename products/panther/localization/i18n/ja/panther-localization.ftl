### Panther localization catalogue (ja).
### Japanese translation of the reference catalogue. Keys mirror the en source.
### Japanese does not mark plural number, so it uses only the CLDR `other`
### category.

## Window chrome.

# The browser window title. The product name stays untranslated.
window-title = Panther

# The action that opens a new browsing tab.
window-new-tab = 新しいタブ

## Permission prompts.

# Title of the camera permission prompt.
# $site (string) is the requesting site. It is isolated for bidi safety.
permissions-camera-title = { $site } にカメラの使用を許可しますか？

# The button that grants a permission request.
permissions-allow = 許可

## Tabs.

# The count of open tabs. $count (number) drives the plural category. $formatted
# (string) is the same count already formatted for the region locale; it is
# isolated for bidi safety. Japanese has no plural distinction, so only the
# `other` category is present.
tabs-open =
    { $count ->
       *[other] { $formatted } 個のタブが開いています
    }

## Capability reasons.
## Each key maps one capability reason. The messages take no arguments so no
## capability-specific value is interpolated.

# A capability is not part of this build.
capability-reason-not-compiled-in = この機能はこのビルドに含まれていません。

# The current platform cannot provide the capability.
capability-reason-platform-unsupported = このプラットフォームはこの機能をサポートしていません。

# The capability is mandatory and stays on for security.
capability-reason-mandatory-security = この機能は必須であり、無効にできません。

# Safe mode turned the capability off.
capability-reason-safe-mode = セーフモードによって機能が無効になりました。

# The capability is experimental and no experiment enabled it.
capability-reason-experiment-gated = この機能は試験的であり、有効にした実験はありません。

# The user turned the capability off.
capability-reason-user-disabled = ユーザーが機能を無効にしました。

# The user turned the capability on.
capability-reason-user-enabled = ユーザーが機能を有効にしました。

# A required capability is not available.
capability-reason-dependency-unmet = 必要な機能が利用できません。

# The capability was quarantined after repeated failures.
capability-reason-quarantined-after-failure = 繰り返し失敗したため、機能は隔離されました。

# The capability is available by default.
capability-reason-default-available = この機能はデフォルトで利用できます。
