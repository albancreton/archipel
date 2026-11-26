# A daemon on another machine, from an app with its own SSH library

For an iOS app, which cannot run the `ssh` command, and for any app that already holds an SSH connection.

`Sources/SSHExample/SSH.swift` is the whole of it: `daemonTransport(over:)` opens a channel that runs `archipel connect --stdio` on the machine and hands its output and its input to the SDK's `StreamTransport`. The SSH connection, its keys and the check of the machine's key stay the app's. Nothing listens on the network: SSH does the encryption and decides who gets in (`docs/remote.md`).

The library is [Citadel](https://github.com/orlandos-nl/Citadel) (on swift-nio-ssh), and its `withExec` needs macOS 15 or iOS 18. This is a package of its own so that the SDK itself carries no SSH library.

```sh
cargo build                          # at the root of the repository
cd examples/swift-ssh && swift test
```

The test starts the system's `sshd` as you, on a port of loopback, with a host key and one allowed key made for the test, and a daemon with the fake agent behind it. No setting of the machine is touched. It checks that:

- a server with another key than the expected one is refused, and so is a key that is not allowed;
- a session is driven through the channel (a prompt, an approval answered) and ends in the reference state, the same as an app on the machine itself reads;
- a second channel on the same SSH connection works;
- a command that does not start is an error, not a wait.

Run on macOS only. It was not run on a phone.
