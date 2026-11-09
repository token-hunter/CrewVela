# CrewVela portable-pty patch

Upstream: portable-pty 0.8.1 (wezterm), copied unchanged except the files below.
Upstream license and package metadata are retained in this directory.

- `src/cmdbuilder.rs`: optional Windows Job handle; omitted from serde data.
- `src/win/procthreadattr.rs`: keep JOB_LIST handle-array storage alive during CreateProcess.
- `src/win/psuedocon.rs`: add JOB_LIST beside PSEUDOCONSOLE, so the process joins its per-runtime Job atomically before execution. Attribute failure rejects spawn.

Unix behavior is unchanged. This is process lifecycle containment, not an OS security sandbox. Job handle ownership remains with CrewVela; the handle is non-inheritable and Job breakaway is not enabled.
