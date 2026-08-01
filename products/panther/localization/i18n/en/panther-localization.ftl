### Panther localization reference catalogue (en).
### This is the source and reference locale. Keys are resource-first and grouped
### by product domain through a hyphenated namespace prefix.

## Window chrome.

# The browser window title.
window-title = Panther

# The action that opens a new browsing tab.
window-new-tab = New Tab

## Permission prompts.

# Title of the camera permission prompt.
# $site (string) is the requesting site. It is isolated for bidi safety.
permissions-camera-title = Allow { $site } to use the camera?

# The button that grants a permission request.
permissions-allow = Allow

## Tabs.

# The count of open tabs. $count (number) drives the plural category. $formatted
# (string) is the same count already formatted for the region locale; it is
# isolated for bidi safety. The number and its display are separate arguments so
# no translated text is concatenated.
tabs-open =
    { $count ->
        [one] One tab is open
       *[other] { $formatted } tabs are open
    }

## Capability reasons.
## The capability system reports typed reasons and stable codes only. The Panther
## adapter maps each reason to one capability-reason-* key here. The messages take
## no arguments so no sensitive value is interpolated.

# A capability is not part of this build.
capability-reason-not-compiled-in = The capability is not included in this build.

# The current platform cannot provide the capability.
capability-reason-platform-unsupported = The platform does not support the capability.

# The capability is mandatory and stays on for security.
capability-reason-mandatory-security = The capability is mandatory and cannot be turned off.

# Safe mode turned the capability off.
capability-reason-safe-mode = Safe mode turned the capability off.

# The capability is experimental and no experiment enabled it.
capability-reason-experiment-gated = The capability is experimental and no experiment enabled it.

# The user turned the capability off.
capability-reason-user-disabled = The user turned the capability off.

# The user turned the capability on.
capability-reason-user-enabled = The user turned the capability on.

# A required capability is not available.
capability-reason-dependency-unmet = A required capability is not available.

# The capability was quarantined after repeated failures.
capability-reason-quarantined-after-failure = The capability was quarantined after repeated failures.

# The capability is available by default.
capability-reason-default-available = The capability is available by default.
