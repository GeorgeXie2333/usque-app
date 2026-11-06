import Foundation

/// The only stdin reader dispatches broker ACKs here while OS/XPC operations run
/// on a separate serial queue. This prevents a callback/read-stdin deadlock.
final class BrokerRelay {
    private final class Pending {
        let ready = DispatchSemaphore(value:0)
        var response:Data?
    }
    private let lock = NSLock()
    private let output = NSLock()
    private var pending:[String:Pending] = [:]
    private var closed = false
    func install() {
        usque_native_xpc_set_callback({ context,_,_,bytes,count in
            guard let context,let bytes else { return nil }
            let bridge = Unmanaged<BrokerRelay>.fromOpaque(context).takeUnretainedValue()
            let response = bridge.request(Data(bytes:bytes,count:count))
            return response.flatMap { String(data:$0,encoding:.utf8).flatMap { strdup($0) } }
        },Unmanaged.passUnretained(self).toOpaque())
    }
    func emit(_ data:Data) throws {
        output.lock(); defer { output.unlock() }; try FrameIO.write(data,to:.standardOutput)
    }
    private func request(_ data:Data)->Data? {
        guard let object = try? JSONSerialization.jsonObject(with:data) as? [String:Any],let id = object["callback_id"] as? String,
              UUID(uuidString:id) != nil else { return nil }
        let result = Pending(); lock.lock()
        guard !closed,pending.count < 16,pending[id] == nil else { lock.unlock(); return nil }
        pending[id] = result; lock.unlock()
        do { try emit(data) } catch { lock.lock(); pending.removeValue(forKey:id); lock.unlock(); return nil }
        let ready = result.ready.wait(timeout:.now()+12) == .success
        lock.lock(); pending.removeValue(forKey:id); let response = ready ? result.response : nil; result.response = nil; lock.unlock()
        return response
    }
    func consume(_ data:Data)->Bool {
        guard let object = try? JSONSerialization.jsonObject(with:data) as? [String:Any],let id = object["callback_id"] as? String,
              object["broker"] is [String:Any] else { return false }
        lock.lock(); defer { lock.unlock() }
        guard let result = pending[id],result.response == nil else { return true }
        result.response = data; result.ready.signal(); return true
    }
    func close() {
        lock.lock(); closed = true; let current = Array(pending.values); pending.removeAll(); lock.unlock()
        current.forEach { $0.ready.signal() }
    }
}
