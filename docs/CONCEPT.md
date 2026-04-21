# Volumio EVO - Concept

Status: concept for the next EVO iteration.
Audience: maintainers.
Vocabulary: load-bearing; used as defined.

## 1. Essence

A device that plays audio from any reachable source, through a configurable audio path, to any present output, while presenting coherent information about what it is doing to any consumer that looks.

Everything in this document is derivable from this sentence plus the fabric vocabulary. Anything in the built system that does not serve this sentence is not essence; it is a plugin contribution or it does not belong.

## 2. Fabric

The product is a STEWARD that administers a CATALOGUE. The catalogue is organised into RACKS. Each rack holds SHELVES. Each shelf has one or more SLOTS of declared SHAPE. PLUGINS stock slots. The steward is the sole authority; plugins never communicate directly.

Every contribution is keyed to one or more SUBJECTS. The steward keeps a canonical SUBJECT REGISTRY that reconciles the many external addressings plugins use (service IDs, file paths, MBIDs, tag-derived identities) into one canonical subject per real thing. Subjects are connected in a governed RELATION GRAMMAR; the steward keeps the resulting graph.

Consumers never address plugins. They address the steward, either by rack (structural query) or by subject (federated query). The steward composes contributions from every rack that has opinions about the subject, walks related subjects within a declared scope, and emits a PROJECTION. All outbound behaviour of the system is either a projection on demand or a streamed HAPPENING on the fabric's notification surface. There is no side channel.

Two classes of originator exist inside the fabric besides external requests. APPOINTMENTS originate actions from time. WATCHES originate actions from observed conditions. Both produce instructions the steward dispatches as if from outside. The CUSTODY LEDGER tracks work entrusted to wardens - capabilities that take custody of long-running operations (playback, mounts, connectivity, file-sharing, kiosk surface). A separate FAST PATH serves real-time mutation (parameter changes, transport commands, volume) without recomposition.

| Fabric concept | One-line role |
|----------------|---------------|
| Essence | The single statement the steward enforces at startup and on admission. |
| Steward | Sole authority. Admits, places, composes, dispatches, projects, notifies. |
| Catalogue | Declared data. Racks, shelves, slots, shapes, relation grammar. |
| Rack | A concern. Holds shelves. Belongs to a family and a kind. |
| Shelf | A slot or slot-set of declared shape within a rack. |
| Slot | A typed opening that admits one or more plugin contributions. |
| Plugin | Any capability that stocks a slot. |
| Subject | A thing the catalogue has opinions about. Canonical identity held by the steward. |
| Relation | Typed directed connection between subjects. |
| Projection | Composed view emitted by the steward, keyed to a rack or a subject. |
| Happening | A transition event the steward emits on its notification stream. |
| Appointment | Time-originated instruction. |
| Watch | Condition-originated instruction. |
| Custody ledger | Registry of active warden assignments and their state. |
| Fast path | Real-time mutation channel alongside the structural slow path. |

## 3. Rack Catalogue

Rack families: DOMAIN (what the product does), COORDINATION (when and why it acts), INFRASTRUCTURE (how the fabric operates over time).

Rack kinds: PRODUCER (originates instructions), TRANSFORMER (moves or changes something), PRESENTER (renders projections to a surface), REGISTRAR (holds knowledge).

A rack may have more than one kind simultaneously when the concern straddles.

| Rack | Family | Kind | Charter |
|------|--------|------|---------|
| Audio | Domain | Transformer | Carry a stream from acquired source to present output. |
| Audio Sources | Domain | Producer | Present selectable things to play; on selection, yield a playable stream. |
| Audio Processing | Domain | Transformer | Alter stream character between acquisition and delivery in declared, composable ways. |
| Networking | Domain | Transformer, Registrar | Move bytes between the device and other addressable things; hold what is reachable. |
| Storage | Domain | Registrar, Transformer | Make content locations reachable; expose them as collections. |
| Library | Domain | Registrar | Unified queryable view across all reachable subject populations. |
| Artwork | Domain | Registrar | Provide visual material representing content. |
| Metadata | Domain | Registrar | Provide factual and descriptive information about content. |
| Branding | Domain | Registrar | Express who this device is across every surface it presents on. |
| Kiosk | Domain | Presenter | Present the device's information to someone in the same room. |
| Appointments | Coordination | Producer | Hold commitments to act at future moments; honour them. |
| Watches | Coordination | Producer | Hold conditions being observed; dispatch when they trigger. |
| Observability | Infrastructure | Registrar | Historical record of the fabric's transitions. |
| Identity | Infrastructure | Registrar | The device's name, locale, time, regulatory stance. |
| Lifecycle | Infrastructure | Producer, Registrar | How the device updates, resets, transfers. |

The list is open. New racks declare new concerns. The steward reads the catalogue as data; no rack is compiled in.

## 4. Plugin Model

Plugins contribute to slots. They never address each other. They address only the steward, through a contract whose shape is declared by the slot they stock.

Two orthogonal axes classify every plugin:

| Axis | Values | Meaning |
|------|--------|---------|
| Instance shape | Singleton, Factory | One contribution forever, or many contributions over time driven by world events. |
| Interaction shape | Respondent, Warden | Answer discrete requests, or take custody of sustained work. |

| Plugin contract item | Required for all | Required for wardens |
|----------------------|------------------|-----------------------|
| Shelf identity and shape version | Yes | Yes |
| Trust class declaration | Yes | Yes |
| Subject addressing and identity claims | Yes | Yes |
| Lifecycle verbs (load, unload) | Yes | Yes |
| State reporting | Yes | Yes |
| Take-custody, release-custody | No | Yes |
| Course-correction verb | No | Yes |
| Failure contract | Lightweight | Full |
| Hot-reload behaviour declaration | Yes | Yes |

"Plugin" does not imply optional, third-party, or sandboxed. It is the universal term for any satisfier of a slot contract. First-party core functionality ships as plugins like everything else. The only entity that is not a plugin is the steward.

## 5. Implementation Commitments

| Commitment | Statement |
|------------|-----------|
| Language | Rust for the steward and for first-party plugins in the trusted tier. |
| Base OS | Debian Trixie minimal (lite). EVO is a layer atop stock Debian, not a rootfs assembled from scratch. |
| Steward process | Single long-running process. Owns the catalogue, the subject registry, the relation graph, the custody ledger, the projection layer, the happenings stream. |
| Plugin delivery | Each plugin is an independently versioned artefact with a declared manifest: shelf target, shape version, trust class, prerequisites, resource requirements. |
| Plugin implementation freedom | A plugin's internal technology is the plugin's choice, governed only by its trust class and the contract it satisfies. Native binaries, sandboxed guests, separate processes, dynamic libraries are all admissible. |
| Trust classes | Declared in manifests. Enforced by the steward. Range from first-party signed privileged wardens to sandboxed untrusted respondents. |
| Versioning | Shelf shapes are versioned. Plugin manifests declare the shape version they satisfy. The steward refuses plugins whose declared version is not in the slot's supported range. |
| Catalogue as data | The rack and shelf declarations are data the steward reads, not code. Adding a new rack is a catalogue edit plus the plugins to stock it. The steward is unchanged. |
| No service knowledge in steward | The steward has no knowledge of Spotify, UPnP, ALSA, nmcli, MPD, Samba, or any specific service, protocol, or subsystem. All such knowledge lives in plugins. |

What is deliberately NOT committed yet: the wire format between steward and out-of-process plugins; the precise manifest schema; the subject identity resolution algorithm; the relation grammar; the projection subscription protocol; the fast-path mechanism. These are engineering questions downstream of this concept.

## 6. Existing Assets

Working code exists for most of the plugin slots this concept calls for. The concept does not discard it. It assigns each functional area a role in the fabric.

| Existing functional area | Fabric role |
|--------------------------|-------------|
| MPD integration and queue | Warden singleton on the audio rack's delivery slot (custody of active playback). |
| Album art resolution with multi-provider fallback chain | Respondent factory on the artwork rack's provider slots; one instance per configured provider. |
| Metavolumio text enrichment (story, bio, credits) | Respondent singleton on the metadata rack's provider slots. |
| Local collection reader (INTERNAL, USB, NAS, SMB roots) | Respondent factory on the audio sources catalogue slot and the library rack; one instance per reachable collection. |
| ALSA configuration and MPD fragment generation | Plugin stocking audio rack stage and delivery slots, with a warden role for the lifecycle of the active output. |
| I2S overlay management and DAC catalogue | Data: catalogue assets on the audio delivery rack. Plugin: trusted warden for boot-partition writes. |
| NetworkManager integration | Warden plugin on the networking rack's link slots. |
| NAS mount orchestration | Warden plugin on the storage rack's mount slots, watching the networking rack for readiness. |
| LAN share discovery | Factory plugin on the storage rack's discovery slot. |
| Samba server management | Warden plugin on the file-sharing slot (peripheral stack; optional). |
| Kiosk control plane and kiosk-browser binary | Warden plugin on the kiosk rack's surface and presenter slots (optional stack). |
| Boot branding installer | Plugin stocking the branding rack's boot-presentation slot. |
| Alarm clock and sleep timer | Appointments rack plugins; originate instructions into the audio rack through the steward. |
| RTC wake programming | Trusted warden supporting the appointments rack. |
| Graceful shutdown/reboot orchestration | Lifecycle plugin; consumes custody ledger to wind down wardens in order. |
| System identity apply (hostname, timezone, regulatory domain) | Identity rack plugins; trusted wardens for the apply half. |
| UI compatibility surface (event-name parity, UIConfig assembly, layout switching) | Temporary projection adapter in the steward's projection layer. Target state: the stock UI consumes fabric projections directly and this adapter retires. |
| Settings persistence (TOML state per feature) | Steward infrastructure. |
| Configuration (TOML + env overrides) | Steward infrastructure. |
| Logging (prefixed journald lines) | Steward infrastructure, plus each plugin's own declared log channel. |
| WASM plugin loader (current form) | One admissible plugin host among several. Not the only one. |

The mapping above is a role assignment, not a reshaping plan. How and when each area is repackaged into its fabric role is a separate engineering document.

## 7. What the Fabric Does Not Do

| Concern | Whose problem |
|---------|---------------|
| Authentication with specific services (OAuth, device-code flows, API keys) | The plugin for that service. |
| Service-specific protocols and codecs | The plugin. |
| File format handling and tag parsing | The plugin that exposes the collection. |
| UI rendering and styling | Consumers of projections (kiosk plugin, remote UI, diagnostic tools). The steward emits projections in a declared structural shape; how they are drawn is not its concern. |
| OS packaging and layer installation | The build system. The fabric is what runs on the device; the layer is how the device becomes the device. |
| Cross-plugin coordination | Does not exist. Plugins cannot coordinate. All composition is through the steward on subject keys. |

## 8. Consequences

- Adding a new service (streaming, discovery protocol, metadata source) is stocking existing shelves with new plugin contributions. The steward is unchanged. The rack list is unchanged. The fabric is unchanged.

- Replacing the playback engine is replacing one warden plugin on one slot. Every other plugin, every other rack, every consumer is unaffected because none of them addresses the playback engine directly.

- Graceful degradation is structural. A consumer asking about a subject receives whatever the fabric can compose; missing contributions mean absent fields, not broken projections.

- Plugin authors never coordinate. The coordination cost of the plugin ecosystem is O(1) per plugin: each author learns the capability contract once.

- The rack list is open; the plugin population is open; the fabric is closed.

## 9. Deliberately Open

These are not gaps. They are concept-level decisions deferred to the engineering layer, named here so the next document knows what it must answer.

| Open question | Decision owner |
|---------------|----------------|
| Wire format between steward and out-of-process plugins | Engineering layer. Candidates include length-prefixed structured messages over Unix sockets, a local message bus, or a narrow HTTP-over-loopback surface. |
| Plugin manifest schema | Engineering layer. Must express shelf target, shape version, trust class, prerequisites, capability declarations. |
| Subject identity resolution | Engineering layer. Must handle many-to-one reconciliation of external addressings, with provenance and user override. |
| Relation grammar | Engineering layer. Must be extensible without steward code changes. |
| Projection subscription protocol | Engineering layer. Must support both pull (query on demand) and push (streamed updates) with rate-limited and aggregated variants. |
| Fast-path mechanism | Engineering layer. Must serve real-time mutation alongside the slow structural path without starvation or reordering. |
| Trust class taxonomy | Engineering layer. Must map trust claims to OS-level privilege boundaries (user, groups, capabilities, seccomp, namespace isolation). |
| Essence enforcement at startup | Engineering layer. Must decide what constitutes "enough" fabric to advertise the device as operational. |
| UI compatibility horizon | Product layer. When the stock UI can consume fabric projections directly, the compatibility adapter retires. Until then, the adapter is part of the steward. |
