### Panther localization catalogue (ar).
### Arabic translation of the reference catalogue. Keys mirror the en source.
### Arabic is written right to left and uses all six CLDR cardinal plural
### categories: zero, one, two, few, many, and other.

## Window chrome.

# The browser window title. The product name stays untranslated.
window-title = Panther

# The action that opens a new browsing tab.
window-new-tab = علامة تبويب جديدة

## Permission prompts.

# Title of the camera permission prompt.
# $site (string) is the requesting site. It is isolated for bidi safety so the
# left-to-right site name stays intact inside the right-to-left sentence.
permissions-camera-title = هل تسمح لـ { $site } باستخدام الكاميرا؟

# The button that grants a permission request.
permissions-allow = السماح

## Tabs.

# The count of open tabs. $count (number) drives the plural category. $formatted
# (string) is the same count already formatted for the region locale; it is
# isolated for bidi safety. Arabic uses all six cardinal categories, so every
# branch is present.
tabs-open =
    { $count ->
        [zero] لا توجد علامات تبويب مفتوحة
        [one] علامة تبويب واحدة مفتوحة
        [two] علامتا تبويب مفتوحتان
        [few] { $formatted } علامات تبويب مفتوحة
        [many] { $formatted } علامة تبويب مفتوحة
       *[other] { $formatted } علامة تبويب مفتوحة
    }

## Capability reasons.
## Each key maps one capability reason. The messages take no arguments so no
## capability-specific value is interpolated.

# A capability is not part of this build.
capability-reason-not-compiled-in = الميزة غير مُضمَّنة في هذه النسخة.

# The current platform cannot provide the capability.
capability-reason-platform-unsupported = النظام الأساسي لا يدعم الميزة.

# The capability is mandatory and stays on for security.
capability-reason-mandatory-security = الميزة إلزامية ولا يمكن إيقافها.

# Safe mode turned the capability off.
capability-reason-safe-mode = أوقف الوضع الآمن الميزة.

# The capability is experimental and no experiment enabled it.
capability-reason-experiment-gated = الميزة تجريبية ولم يُفعّلها أي اختبار.

# The user turned the capability off.
capability-reason-user-disabled = أوقف المستخدم الميزة.

# The user turned the capability on.
capability-reason-user-enabled = فعّل المستخدم الميزة.

# A required capability is not available.
capability-reason-dependency-unmet = إحدى الميزات المطلوبة غير متوفرة.

# The capability was quarantined after repeated failures.
capability-reason-quarantined-after-failure = عُزلت الميزة بعد أعطال متكررة.

# The capability is available by default.
capability-reason-default-available = الميزة متوفرة افتراضيًا.
