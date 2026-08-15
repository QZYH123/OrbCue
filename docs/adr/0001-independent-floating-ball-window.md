# Use an Independent Floating Window for the Collapsed Ball

The collapsed Dock will be a separate tiny always-on-top window rather than a reshaped terminal window. Terminal emulators do not share a reliable window-shaping interface, while an independent window works across terminals and can focus the originating terminal on click or hotkey.

## Consequences

The Dock must retain a way to focus the originating terminal, and the first presenter depends on a desktop display. Terminal-only environments can add a different presenter later without changing the state model.
