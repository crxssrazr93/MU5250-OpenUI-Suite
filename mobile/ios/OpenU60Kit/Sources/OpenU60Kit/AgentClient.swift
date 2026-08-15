import Foundation

#if canImport(FoundationNetworking)
import FoundationNetworking
#endif

/// Everything the agent answers is wrapped in this.
public struct AgentEnvelope<T: Decodable>: Decodable {
    public let ok: Bool
    public let data: T?
    public let error: String?
}

public enum AgentError: Error, LocalizedError, Equatable {
    case notConfigured
    case unauthorized
    case notFound(String)
    case server(String)
    case transport(String)
    case timedOut
    case decoding(String)

    public var errorDescription: String? {
        switch self {
        case .notConfigured: return "No router address set"
        case .unauthorized: return "Session expired"
        case .notFound(let path): return "The agent does not serve \(path)"
        case .server(let message): return message
        case .transport(let message): return "Could not reach the agent: \(message)"
        case .timedOut: return "Timed out reaching the agent"
        case .decoding(let message): return "Unexpected response: \(message)"
        }
    }
}

/// Talks to the on-device agent.
///
/// Deliberately small. The agent is on the LAN over plain HTTP with a bearer
/// token, and there is no second backend to abstract over, so there is nothing
/// here but request building, the envelope, and the two timeouts.
public actor AgentClient {
    /// Routes that change the device require this, and the agent rejects them
    /// without it. Kept here so no caller can forget it on one screen.
    static let confirmHeader = "X-Confirm"

    /// Ordinary requests. Anything slower than this is the firmware being
    /// unwell, and failing fast is better than a spinner that never stops.
    private let standardTimeout: TimeInterval = 15

    /// Card and RSP work. An eSIM download is a multi-leg conversation with the
    /// operator's SM-DP+ and every card operation is serialized behind one
    /// mutex on the agent, so fifteen seconds is not close to enough.
    private let slowTimeout: TimeInterval = 360

    private let session: URLSession
    private var host: String?
    private var token: String?

    public init(session: URLSession = .shared) {
        self.session = session
    }

    /// The router to talk to. Rejects anything outside the private ranges, the
    /// same rule the agent applies to origins — the app has no business
    /// reaching an agent across the internet, and a typo should not try.
    @discardableResult
    public func setHost(_ value: String) -> Bool {
        let candidate = value.trimmingCharacters(in: .whitespaces)
        guard Self.isPrivateAddress(candidate) else { return false }
        host = candidate
        return true
    }

    public func currentHost() -> String? { host }

    public func setToken(_ value: String?) { token = value }

    public func hasToken() -> Bool { token != nil }

    public var baseURL: URL? {
        guard let host else { return nil }
        return URL(string: "http://\(host):9090")
    }

    // MARK: - Requests

    public func get<T: Decodable>(_ path: String, slow: Bool = false) async throws -> T {
        try await request(method: "GET", path: path, body: nil, slow: slow)
    }

    public func post<T: Decodable>(
        _ path: String, body: [String: Any]? = nil, confirmed: Bool = false, slow: Bool = false
    ) async throws -> T {
        try await request(method: "POST", path: path, body: body, confirmed: confirmed, slow: slow)
    }

    public func put<T: Decodable>(
        _ path: String, body: [String: Any]? = nil, confirmed: Bool = false
    ) async throws -> T {
        try await request(method: "PUT", path: path, body: body, confirmed: confirmed)
    }

    /// Sign in and keep the token.
    public func login(password: String? = nil, pin: String? = nil) async throws {
        var body: [String: Any] = [:]
        if let password { body["password"] = password }
        if let pin { body["pin"] = pin }
        let result: LoginResult = try await request(
            method: "POST", path: "/api/auth/login", body: body
        )
        token = result.token
    }

    private struct LoginResult: Decodable { let token: String }

    private func request<T: Decodable>(
        method: String,
        path: String,
        body: [String: Any]?,
        confirmed: Bool = false,
        slow: Bool = false
    ) async throws -> T {
        guard let baseURL, let url = URL(string: path, relativeTo: baseURL) else {
            throw AgentError.notConfigured
        }

        var request = URLRequest(url: url)
        request.httpMethod = method
        request.timeoutInterval = slow ? slowTimeout : standardTimeout
        if let token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        if confirmed {
            request.setValue("true", forHTTPHeaderField: Self.confirmHeader)
        }
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONSerialization.data(withJSONObject: body)
        }

        let payload: Data
        let response: URLResponse
        do {
            (payload, response) = try await session.data(for: request)
        } catch let error as URLError where error.code == .timedOut {
            throw AgentError.timedOut
        } catch {
            throw AgentError.transport(error.localizedDescription)
        }

        let status = (response as? HTTPURLResponse)?.statusCode ?? 0
        if status == 401 {
            token = nil
            throw AgentError.unauthorized
        }

        let envelope: AgentEnvelope<T>
        do {
            envelope = try JSONDecoder().decode(AgentEnvelope<T>.self, from: payload)
        } catch {
            // A body that is not the envelope at all means the request never
            // reached the agent's router — a proxy, a captive portal, or the
            // vendor web server on the wrong port.
            throw AgentError.decoding("HTTP \(status)")
        }

        guard envelope.ok, let data = envelope.data else {
            let message = envelope.error ?? "request failed (\(status))"
            // Told apart from a real failure because it is the answer to "does
            // this firmware support X", which several screens ask.
            if message.lowercased() == "not found" || status == 404 {
                throw AgentError.notFound(path)
            }
            throw AgentError.server(message)
        }
        return data
    }

    // MARK: - Address rules

    /// Loopback or RFC1918, matching the agent's own view of what is local.
    ///
    /// Addresses only, not names: a hostname resolves wherever DNS says, which
    /// would put the check somewhere it cannot be made.
    public static func isPrivateAddress(_ value: String) -> Bool {
        if value == "localhost" || value == "::1" { return true }
        let parts = value.split(separator: ".", omittingEmptySubsequences: false)
        guard parts.count == 4 else { return false }
        var octets: [Int] = []
        for part in parts {
            guard !part.isEmpty, part.count <= 3, part.allSatisfy({ $0.isASCII && $0.isNumber }),
                  let value = Int(part), value <= 255 else { return false }
            octets.append(value)
        }
        switch (octets[0], octets[1]) {
        case (10, _), (127, _), (192, 168): return true
        case (172, 16...31): return true
        default: return false
        }
    }
}
