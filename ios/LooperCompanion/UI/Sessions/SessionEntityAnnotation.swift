import AppIntents
import SwiftUI

#if canImport(_AppIntents_SwiftUI)
import _AppIntents_SwiftUI
#endif

extension View {
    @ViewBuilder
    func looperAppEntityIdentifier(_ identifier: EntityIdentifier?) -> some View {
        #if canImport(_AppIntents_SwiftUI)
        if #available(iOS 18.4, macOS 15.4, watchOS 11.4, tvOS 18.4, visionOS 2.4, *) {
            appEntityIdentifier(identifier)
        } else {
            self
        }
        #else
        self
        #endif
    }
}
