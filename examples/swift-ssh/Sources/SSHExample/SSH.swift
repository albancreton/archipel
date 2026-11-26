// A daemon on another machine, from an app that has an SSH library of its own. On iOS that is
// the only way: an app there cannot run the `ssh` command.
//
// The app opens the SSH connection (its keys, its way of checking the machine's key) and a
// channel that runs `archipel connect --stdio` on the machine. `StreamTransport` then speaks to
// the daemon on what the channel prints and takes. Nothing listens on the network for this: SSH
// does the encryption and decides who gets in (docs/remote.md).
//
// The library here is Citadel, which is built on swift-nio-ssh. With another one, the same
// three things are handed to `StreamTransport`: the bytes that arrive, a way to write, a way to
// close.

import ArchipelClient
import Citadel
import Foundation
import NIOCore
import NIOFoundationCompat

/// The channel was closed before it could be used: the command did not start.
public struct ChannelDidNotOpen: Error {}

/// Runs `command` on the machine `ssh` is connected to, and gives the connection to the daemon
/// it carries. Closing the transport closes the channel; the SSH connection stays the app's.
///
/// ```swift
/// let client = ArchipelClient(name: "MyApp") {
///     try await daemonTransport(over: ssh)
/// }
/// ```
///
/// A command run through SSH does not have a terminal's `PATH`: name the program in full when
/// it is not found (`.archipel/bin/archipel connect --stdio`).
public func daemonTransport(over ssh: SSHClient, command: String = "archipel connect --stdio") async throws -> StreamTransport {
    let (arriving, arrived) = AsyncStream<Data>.makeStream()
    let (opening, opened) = AsyncThrowingStream<TTYStdinWriter, Error>.makeStream()
    // The channel lives as long as this task: `withExec` closes it when its body returns.
    let channel = Task {
        do {
            try await ssh.withExec(command) { output, input in
                opened.yield(input)
                for try await piece in output {
                    // What the command prints on its error output is not part of the connection.
                    if case .stdout(let bytes) = piece { arrived.yield(Data(buffer: bytes)) }
                }
            }
            opened.finish()
        } catch {
            opened.finish(throwing: error)
        }
        arrived.finish()
    }
    var first = opening.makeAsyncIterator()
    guard let input = try await first.next() else { throw ChannelDidNotOpen() }
    return StreamTransport(
        reading: arriving,
        write: { try await input.write(ByteBuffer(data: $0)) },
        close: { channel.cancel() })
}
