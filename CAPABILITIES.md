# lazylibrarian — ServiceBackend contract checklist

Pure-Rust plugin: **no bash, no compose, no provision scripts**. Driven by the
generic `service.*` surface (no per-plugin tools). Modalities: **docker,podman,lxc**.

## What this plugin implements (the only per-plugin work)
- [ ] `provider` / `runtimes` / `default_port` / `capabilities` / `data_paths` — declarative
- [ ] `workload_spec(runtime)` — *what* to run; `deploy_target` renders it to compose/LXC/VM
- [ ] `configure` — service-specific config via the upstream API
- [ ] `status` — health/diagnostics

## Inherited generically (NO code in this plugin)
- `deploy` — `service.deploy` → `deploy_target.launch(WorkloadSpec)`
- `backup` / `restore` — pluggable `BackupMethod` (tar for containers/LXC, **PBS** for Proxmox guests when available)
- `connect` / `sync` — endpoint registry + peer sync in the toolkit
