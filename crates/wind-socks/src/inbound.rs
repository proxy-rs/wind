struct Opt {
    /// Bind on address address. eg. `127.0.0.1:1080`
    pub listen_addr: String,

    /// Our external IP address to be sent in reply packets (required for UDP)
    pub public_addr: Option<std::net::IpAddr>,

    /// Request timeout
    pub request_timeout: u64,

    /// Choose authentication type
    pub auth: AuthMode,

    /// Don't perform the auth handshake, send directly the command request
    pub skip_auth: bool,

    /// Allow UDP proxying, requires public-addr to be set
    pub allow_udp: bool,
}

enum AuthMode {
    NoAuth,
    Password {
        username: String,
        password: String,
    },
}