import Darwin
import Foundation
import Synchronization
import Testing

@testable import MenagerieCore

private final class SlowHTTPServer {
  let descriptor: Int32
  let url: String
  init() throws {
    let fd = socket(AF_INET, SOCK_STREAM, 0)
    descriptor = fd
    guard descriptor >= 0 else { throw FleetError("socket") }
    var address = sockaddr_in()
    address.sin_family = sa_family_t(AF_INET)
    address.sin_addr.s_addr = inet_addr("127.0.0.1")
    let bound = withUnsafePointer(to: &address) {
      $0.withMemoryRebound(to: sockaddr.self, capacity: 1) {
        bind(fd, $0, socklen_t(MemoryLayout<sockaddr_in>.size))
      }
    }
    guard bound == 0, listen(descriptor, 1) == 0 else { throw FleetError("bind") }
    var length = socklen_t(MemoryLayout<sockaddr_in>.size)
    _ = withUnsafeMutablePointer(to: &address) {
      $0.withMemoryRebound(to: sockaddr.self, capacity: 1) { getsockname(fd, $0, &length) }
    }
    url = "http://127.0.0.1:\(UInt16(bigEndian: address.sin_port))"
    let listener = descriptor
    Thread.detachNewThread {
      let client = accept(listener, nil, nil)
      guard client >= 0 else { return }
      defer { close(client) }
      var one: Int32 = 1
      setsockopt(client, SOL_SOCKET, SO_NOSIGPIPE, &one, socklen_t(MemoryLayout<Int32>.size))
      var request = [UInt8](repeating: 0, count: 4096)
      _ = read(client, &request, request.count)
      let headers = Array(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 10000\r\nConnection: close\r\n\r\n".utf8)
      _ = headers.withUnsafeBytes { write(client, $0.baseAddress!, $0.count) }
      var byte: UInt8 = 32
      for _ in 0..<100 {
        usleep(20000)
        if write(client, &byte, 1) < 0 { break }
      }
    }
  }
  deinit {
    shutdown(descriptor, SHUT_RDWR)
    close(descriptor)
  }
}
@Test func serviceHealthUsesHeadersWithoutDrainingStreamingBody() async throws {
  let server = try SlowHTTPServer()
  let session = URLSession(configuration: .ephemeral)
  defer { session.invalidateAndCancel() }
  let fleet = try Fleet.parse(
    "version: 1\nmachines: {test: {services: [{name: stream, url: '\(server.url)', health: '\(server.url)'}]}}"
  )
  let start = Date()
  let result = await Poller.poll(fleet, known: [:], session: session)
  #expect(
    result.health["test/0"]?.hasPrefix("up ·") == true, Comment(rawValue: result.health.description)
  )
  #expect(Date().timeIntervalSince(start) < 1.5)
}
@Test func streamingAgentsHaveATotalDeadline() async throws {
  let server = try SlowHTTPServer()
  let session = URLSession(configuration: .ephemeral)
  defer { session.invalidateAndCancel() }
  let fleet = try Fleet.parse("version: 1\nmachines: {test: {agent: '\(server.url)'}}")
  let start = Date()
  let result = await Poller.poll(
    fleet, known: [:], session: session, agentDeadline: .milliseconds(100))
  #expect(result.observations["test"]?.error?.contains("total deadline") == true)
  #expect(Date().timeIntervalSince(start) < 1.5)
}
private actor HTTPGate {
  private var continuation: CheckedContinuation<Void, Never>?
  private var released = false
  func wait() async { if !released { await withCheckedContinuation { continuation = $0 } } }
  func release() {
    released = true
    continuation?.resume()
  }
}
@Test func healthPublishesBeforeBlockedGitComparisonFinishes() async throws {
  let server = try SlowHTTPServer()
  let session = URLSession(configuration: .ephemeral)
  defer { session.invalidateAndCancel() }
  let fleet = try Fleet.parse(
    "version: 1\nrepo: /unused\nmachines: {test: {services: [{name: stream, url: '\(server.url)', health: '\(server.url)'}]}}"
  )
  let gate = HTTPGate()
  let finished = Mutex(false)
  let published = Mutex(false)
  let fallback = Task {
    try? await Task.sleep(for: .seconds(1))
    await gate.release()
  }
  defer { fallback.cancel() }
  _ = await Poller.poll(
    fleet, known: [:], session: session,
    onHealth: { health in
      #expect(!finished.withLock { $0 })
      published.withLock { $0 = health["test/0"]?.hasPrefix("up") == true }
      await gate.release()
    },
    comparison: { _, _, _ in
      await gate.wait()
      finished.withLock { $0 = true }
      return PollResult()
    })
  #expect(published.withLock { $0 })
}
