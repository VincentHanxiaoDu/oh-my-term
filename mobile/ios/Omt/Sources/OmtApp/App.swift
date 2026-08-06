// The app itself.
//
// Small on purpose: an entry point that does anything interesting is an entry
// point that cannot be tested, and everything interesting is in `Client` and
// the views, which the checks drive without a window.

import SwiftUI

/// The application.
///
/// `@main` lives behind a flag because this target is also compiled as a
/// library on machines without Xcode — which is where the checks run, and
/// where most mistakes are actually found. With `OMT_APP` set, the same code
/// is the app's entry point.
#if OMT_APP
@main
#endif
public struct OmtApplication: App {
    @StateObject private var client = Client()
    @AppStorage("omt.instance") private var instance = ""
    @AppStorage("omt.token") private var token = ""

    public init() {}

    public var body: some Scene {
        WindowGroup {
            if let url = URL(string: instance), !token.isEmpty {
                RosterView(client: client)
                    .task { client.connect(to: url, token: token) }
            } else {
                // No credential, so the first screen is the one that gets one.
                // Opening to an empty roster and leaving somebody to find
                // settings is how an app looks broken on first launch.
                ConnectView(instance: $instance, token: $token)
            }
        }
    }
}

/// Where an instance and its token are entered.
public struct ConnectView: View {
    @Binding var instance: String
    @Binding var token: String

    public init(instance: Binding<String>, token: Binding<String>) {
        _instance = instance
        _token = token
    }

    public var body: some View {
        Form {
            Section {
                TextField("https://box.local:7777", text: $instance)
                    .textContentType(.URL)
                    #if os(iOS)
                    .keyboardType(.URL)
                    .textInputAutocapitalization(.never)
                    #endif
                    .autocorrectionDisabled()
                // Secure, because this is the whole authority over somebody's
                // shell and it should not be sitting in a screenshot.
                SecureField("token", text: $token)
            } header: {
                Text("Your instance")
            } footer: {
                Text("`omt web` prints both, once.")
            }
        }
    }
}
