// Where plugins and libraries come from.
//
// Declared here rather than in the module, because Gradle resolves the plugin
// block before anything in the project runs — a repository added later is one
// the plugin resolution never sees.
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "omt"
include(":app")
