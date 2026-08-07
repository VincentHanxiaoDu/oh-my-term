// The application's entry point.
//
// A file of its own so `@main` lives in exactly one place: the app target has
// it, and `OmtApp` stays a library that compiles anywhere — including on a
// machine with no Xcode, which is where the sixteen checks run.

import OmtApp

OmtApplication.main()
