### Panther localization catalogue (es).
### Spanish translation of the reference catalogue. Keys mirror the en source.
### Spanish cardinal plurals use the CLDR `one` and `other` categories.

## Window chrome.

# The browser window title. The product name stays untranslated.
window-title = Panther

# The action that opens a new browsing tab.
window-new-tab = Nueva pestaña

## Toolbar.

# The action that navigates to the previous page in history.
toolbar-back = Atrás

# The action that navigates to the next page in history.
toolbar-forward = Adelante

# The action that reloads the current page.
toolbar-reload = Recargar

# The placeholder text in the empty address field.
address-placeholder = Buscar o escribir dirección

## Permission prompts.

# Title of the camera permission prompt.
# $site (string) is the requesting site. It is isolated for bidi safety.
permissions-camera-title = ¿Permitir que { $site } use la cámara?

# The button that grants a permission request.
permissions-allow = Permitir

## Tabs.

# The count of open tabs. $count (number) drives the plural category. $formatted
# (string) is the same count already formatted for the region locale; it is
# isolated for bidi safety. Spanish uses the `one` and `other` categories.
tabs-open =
    { $count ->
        [one] Hay una pestaña abierta
       *[other] Hay { $formatted } pestañas abiertas
    }

## Capability reasons.
## Each key maps one capability reason. The messages take no arguments so no
## capability-specific value is interpolated.

# A capability is not part of this build.
capability-reason-not-compiled-in = La funcionalidad no se incluye en esta compilación.

# The current platform cannot provide the capability.
capability-reason-platform-unsupported = La plataforma no admite la funcionalidad.

# The capability is mandatory and stays on for security.
capability-reason-mandatory-security = La funcionalidad es obligatoria y no se puede desactivar.

# Safe mode turned the capability off.
capability-reason-safe-mode = El modo seguro desactivó la funcionalidad.

# The capability is experimental and no experiment enabled it.
capability-reason-experiment-gated = La funcionalidad es experimental y ningún experimento la activó.

# The user turned the capability off.
capability-reason-user-disabled = El usuario desactivó la funcionalidad.

# The user turned the capability on.
capability-reason-user-enabled = El usuario activó la funcionalidad.

# A required capability is not available.
capability-reason-dependency-unmet = Una funcionalidad necesaria no está disponible.

# The capability was quarantined after repeated failures.
capability-reason-quarantined-after-failure = La funcionalidad se puso en cuarentena tras fallos repetidos.

# The capability is available by default.
capability-reason-default-available = La funcionalidad está disponible de forma predeterminada.
