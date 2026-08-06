// The plugin versions, in one place. Declared with `apply false` so the root
// project resolves them and the module below applies them — the arrangement
// Android tooling expects, and the one that stops a version drifting between
// modules when a second one is added.
plugins {
    id("com.android.application") version "8.5.2" apply false
    kotlin("android") version "1.9.24" apply false
}
