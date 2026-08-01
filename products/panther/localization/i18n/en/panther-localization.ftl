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
## Placeholder namespace filled in a later phase. The capability system reports
## typed reasons; the Panther adapter maps them to capability-reason-* keys here.
