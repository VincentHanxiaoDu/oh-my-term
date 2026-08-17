# mobile Specification

## ADDED Requirements

### Requirement: The configured media quota is the quota

The blob store SHALL be built from `media.quota.*`.

It was built from the built-in defaults, so somebody who raised their blob
limit still had a screenshot refused at thirty-two megabytes by a number they
had changed and could see was changed.

#### Scenario: A raised limit

- **WHEN** `media.quota.max_blob_bytes` is larger than the default and a blob
  under it is stored
- **THEN** it is accepted

#### Scenario: An unreadable size

- **WHEN** a size setting cannot be read
- **THEN** the default applies and omt starts normally

### Requirement: Images can be turned off

When images are disabled, omt SHALL NOT query the terminal about graphics.

The switch could not turn anything off: the image path was unconditional, and
`protocol = "none"` drew images anyway.

#### Scenario: Disabled

- **WHEN** `media.images.enabled` is `false`, or the protocol is `none`
- **THEN** no graphics query is written, and the terminal works normally
