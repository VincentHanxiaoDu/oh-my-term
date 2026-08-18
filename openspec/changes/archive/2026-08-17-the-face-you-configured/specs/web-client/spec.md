# web-client Specification

## ADDED Requirements

### Requirement: The theme is the configured one

`theme.get` SHALL serve the theme `appearance.theme_mode` names — the light
half for `light`, the dark half otherwise — with `appearance.theme_light`
naming the light half.

#### Scenario: Light mode

- **WHEN** `appearance.theme_mode` is `light`
- **THEN** the light theme is served

#### Scenario: Dark and auto

- **WHEN** the mode is `dark` or `auto`
- **THEN** the dark theme is served, as it always was

### Requirement: The appearance settings render

The web client SHALL apply the `appearance.*` and `font.*` settings it is
given, with the previous rendering as the default when none are present.

#### Scenario: A configured face

- **WHEN** a font family and size are configured
- **THEN** the terminal renders them

#### Scenario: Nothing configured

- **WHEN** no appearance settings resolve
- **THEN** the client renders exactly what it always did
