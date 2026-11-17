import Foundation
import NetworkExtension

@main struct ProviderMain {
    static func main() {
        NEProvider.startSystemExtensionMode()
        dispatchMain()
    }
}
