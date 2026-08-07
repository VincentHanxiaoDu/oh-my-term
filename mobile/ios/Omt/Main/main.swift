// The application's entry point.
//
// The app target compiles the library sources directly, so there is no module
// to import — `OmtApplication` is in this target. `swift build` still compiles
// the same files as a library, which is what keeps the sixteen checks runnable
// on a machine with no Xcode at all.

import SwiftUI

OmtApplication.main()
