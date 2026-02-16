/**
 * Admin Templates Visual Editor
 * Handles the logic for converting intuitive form inputs into Sing-box JSON structure.
 * Supports multiple instances (Create Form, Edit Modal) by scoping selectors.
 */

class VisualEditor {
    constructor(container) {
        if (!container) {
            console.error("VisualEditor initialized without container");
            return;
        }
        this.container = container;
        this.container.visualEditor = this;
        this.state = {
            mode: 'visual', // 'visual' | 'manual'
            protocol: 'vless',
            network: 'tcp',
            security: 'reality'
        };

        this.init();
    }

    init() {
        // 1. Hook up Mode Toggle
        const modeToggle = this.container.querySelector('.editor-mode-toggle');
        if (modeToggle) {
            modeToggle.addEventListener('change', (e) => {
                this.state.mode = e.target.checked ? 'manual' : 'visual';
                this.toggleEditorMode();
            });
        }

        // 1.5 Sync with Master Protocol Selector (fixes duplication)
        const masterProtocol = this.container.querySelector('select[name="protocol"]');
        const internalProtocol = this.container.querySelector('.vis-protocol');
        const internalContainer = this.container.querySelector('.vis-protocol-container');

        if (masterProtocol && internalProtocol && masterProtocol !== internalProtocol) {
            // Hide the internal one if a master one exists
            if (internalContainer) internalContainer.classList.add('hidden');

            // Sync values initially
            internalProtocol.value = masterProtocol.value;

            // Listen to master
            masterProtocol.addEventListener('change', () => {
                internalProtocol.value = masterProtocol.value;
                this.updateVisualState();
            });
        }

        // 2. Hook up Visual Controls
        this.container.querySelector('.vis-protocol')?.addEventListener('change', () => this.updateVisualState());
        this.container.querySelector('.vis-network')?.addEventListener('change', () => this.updateVisualState());
        this.container.querySelector('.vis-security')?.addEventListener('change', () => this.updateVisualState());

        // 3. Hook up Inputs for live generation
        const inputs = this.container.querySelectorAll('input, select');
        inputs.forEach(input => {
            if (!input.classList.contains('editor-mode-toggle')) {
                input.addEventListener('change', () => {
                    if (this.state.mode === 'visual') this.generateJsonFromVisual();
                });
                input.addEventListener('input', () => {
                    if (this.state.mode === 'visual') this.generateJsonFromVisual();
                });
            }
        });

        // 4. Hook Form Submit for Validation
        // Find the parent form
        const form = this.container.closest('form') || this.container.querySelector('form');
        if (form) {
            form.addEventListener('submit', (e) => {
                if (this.state.mode === 'visual') {
                    if (!this.validateVisualConfig()) {
                        e.preventDefault();
                        e.stopPropagation();
                        // alert("Please fix validation errors in the Visual Editor before saving.");
                    } else {
                        // Ensure JSON is up to date
                        this.generateJsonFromVisual();
                    }
                }
            });
        }

        // 5. Initial State
        // Try parsing existing JSON first (for Edit mode)
        // If parsing fails (e.g. empty new template), force generation from defaults
        if (!this.parseJsonToVisual() && this.state.mode === 'visual') {
            this.updateVisualState();
        }
    }

    toggleEditorMode() {
        const visualContainer = this.container.querySelector('.visual-editor-container');
        const manualContainer = this.container.querySelector('.manual-editor-container');

        if (this.state.mode === 'visual') {
            visualContainer?.classList.remove('hidden');
            manualContainer?.classList.add('hidden');
            // When switching to Visual, try to parse what's in the Manual textareas
            this.parseJsonToVisual();
        } else {
            visualContainer?.classList.add('hidden');
            manualContainer?.classList.remove('hidden');
            // When switching to Manual, sync Visual data TO the textareas
            this.generateJsonFromVisual();
        }
    }

    updateVisualState() {
        // Get values
        const pEl = this.container.querySelector('.vis-protocol');
        const nEl = this.container.querySelector('.vis-network');
        const sEl = this.container.querySelector('.vis-security');

        if (!pEl || !nEl || !sEl) return;

        this.state.protocol = pEl.value;
        this.state.network = nEl.value;
        this.state.security = sEl.value;

        const p = this.state.protocol;
        const n = this.state.network;
        const s = this.state.security;

        // Show/Hide relevant sections
        const toggle = (selector, condition) => {
            const el = this.container.querySelector(selector);
            if (el) condition ? el.classList.remove('hidden') : el.classList.add('hidden');
        };

        toggle('.client-section', ['vless', 'vmess', 'trojan', 'tuic', 'hysteria2', 'naive'].includes(p));
        toggle('.flow-section', p === 'vless' && s === 'reality' && n === 'tcp');

        // Transports
        this.container.querySelectorAll('.transport-settings').forEach(el => el.classList.add('hidden'));
        const transportMap = {
            'tcp': '.tcp-settings', // usually none
            'ws': '.ws-settings',
            'grpc': '.grpc-settings',
            'httpupgrade': '.httpupgrade-settings',
            'quic': '.quic-settings'
        };
        if (transportMap[n]) toggle(transportMap[n], true);

        // Security
        toggle('.tls-settings', s === 'tls');
        toggle('.reality-settings', s === 'reality');

        // Sync Protocol to Main Form Hidden Input (if exists)
        // Usually handled by generated JSON, but for top-level protocol field:
        const mainProtocolInput = this.container.querySelector('select[name="protocol"]');
        if (mainProtocolInput && !mainProtocolInput.classList.contains('vis-protocol')) {
            mainProtocolInput.value = p;
        }

        if (this.state.mode === 'visual') {
            this.generateJsonFromVisual();
        }
    }

    generateJsonFromVisual() {
        if (this.state.mode === 'manual') return; // CRITICAL: Do not overwrite manual edits!

        const protocol = this.container.querySelector('.vis-protocol').value;
        const network = this.container.querySelector('.vis-network').value;
        const security = this.container.querySelector('.vis-security').value;

        // --- Settings JSON ---
        let settings = {
            protocol: protocol,
            clients: [] // Default empty
        };

        // Basic placeholder logic
        if (['vless', 'vmess'].includes(protocol)) {
            settings.clients.push({
                id: "{{uuid}}",
                flow: (protocol === 'vless' && security === 'reality' && network === 'tcp') ? "xtls-rprx-vision" : undefined
            });
            settings.decryption = "none";
            settings.fallbacks = [];
        } else if (protocol === 'trojan') {
            settings.clients.push({ password: "{{uuid}}" });
            settings.fallbacks = [];
        } else if (protocol === 'shadowsocks') {
            settings.method = "2022-blake3-aes-128-gcm";
            settings.password = "{{uuid}}";
            settings.network = "tcp,udp";
            delete settings.clients;
        } else if (protocol === 'hysteria2') {
            settings.password = "{{uuid}}";
            delete settings.clients;
        } else if (protocol === 'tuic') {
            settings.users = [{ uuid: "{{uuid}}", password: "{{uuid}}" }];
            settings.congestion_control = "bbr";
            settings.heartbeat = "10s";
            delete settings.clients;
        } else if (protocol === 'naive') {
            settings.users = [{ username: "{{uuid}}", password: "{{uuid}}" }];
            settings.network = "tcp";
            delete settings.clients;
        }

        // Clean undefined in clients
        if (settings.clients) {
            settings.clients = settings.clients.map(c => {
                const clean = {};
                for (const key in c) {
                    if (c[key] !== undefined) clean[key] = c[key];
                }
                return clean;
            });
        }

        // Handle both settings_template (preferred) and settings (fallback) names
        const settingsArea = this.container.querySelector('textarea[name="settings_template"]') || this.container.querySelector('textarea[name="settings"]');
        if (settingsArea) settingsArea.value = JSON.stringify(settings, null, 2);


        // --- Stream Settings JSON ---
        let stream = {
            network: network,
            security: security
        };

        if (network === 'ws') {
            stream.wsSettings = {
                path: this.getVal('.vis-ws-path', '/'),
                headers: { Host: this.getVal('.vis-ws-host', '{{sni}}') }
            };
        } else if (network === 'grpc') {
            stream.grpcSettings = {
                serviceName: this.getVal('.vis-grpc-service', 'grpc')
            };
        } else if (network === 'httpupgrade') {
            stream.httpUpgradeSettings = {
                path: this.getVal('.vis-http-path', '/'),
                host: this.getVal('.vis-http-host', '{{sni}}')
            }
        }

        if (security === 'tls') {
            stream.tlsSettings = {
                serverName: this.getVal('.vis-tls-sni', '{{sni}}'),
                certificates: [{ certificateFile: "/etc/ssl/certs/cert.pem", keyFile: "/etc/ssl/private/key.pem" }]
            };
        } else if (security === 'reality') {
            stream.realitySettings = {
                show: false,
                dest: this.getVal('.vis-reality-dest', 'www.google.com:443'),
                serverNames: this.getVal('.vis-reality-sni', 'www.google.com').split(','),
                privateKey: "{{reality_private}}", // Always use placeholder for template
                shortIds: ["", "0123456789abcdef"]
            };
            if (network === 'tcp') stream.realitySettings.xver = 0;
        }

        const streamArea = this.container.querySelector('textarea[name="stream_settings_template"]') || this.container.querySelector('textarea[name="stream_settings"]');
        if (streamArea) streamArea.value = JSON.stringify(stream, null, 2);
    }

    parseJsonToVisual() {
        try {
            const settingsArea = this.container.querySelector('textarea[name="settings_template"]') || this.container.querySelector('textarea[name="settings"]');
            const streamArea = this.container.querySelector('textarea[name="stream_settings_template"]') || this.container.querySelector('textarea[name="stream_settings"]');

            if (!settingsArea || !streamArea || !settingsArea.value || !streamArea.value) return false;

            const settings = JSON.parse(settingsArea.value);
            const stream = JSON.parse(streamArea.value);

            // Protocol
            if (settings.protocol) {
                const el = this.container.querySelector('.vis-protocol');
                if (el) el.value = settings.protocol;
            }

            // Network
            if (stream.network) {
                const el = this.container.querySelector('.vis-network');
                if (el) el.value = stream.network;
            }

            // Security
            if (stream.security) {
                const el = this.container.querySelector('.vis-security');
                if (el) el.value = stream.security;
            }

            // Populate Fields
            if (stream.wsSettings) {
                this.setVal('.vis-ws-path', stream.wsSettings.path);
                this.setVal('.vis-ws-host', stream.wsSettings.headers?.Host);
            }
            if (stream.grpcSettings) {
                this.setVal('.vis-grpc-service', stream.grpcSettings.serviceName);
            }
            if (stream.httpUpgradeSettings) {
                this.setVal('.vis-http-path', stream.httpUpgradeSettings.path);
                this.setVal('.vis-http-host', stream.httpUpgradeSettings.host);
            }

            if (stream.realitySettings) {
                this.setVal('.vis-reality-dest', stream.realitySettings.dest);
                this.setVal('.vis-reality-sni', (stream.realitySettings.serverNames || []).join(','));
            }
            if (stream.tlsSettings) {
                this.setVal('.vis-tls-sni', stream.tlsSettings.serverName);
            }

            this.updateVisualState();
            return true;
        } catch (e) {
            console.warn("VisualEditor: Failed to parse JSON to Visual", e);

            // If we're already in visual mode but parsing fails (shouldn't happen on switch, but maybe on init),
            // show a small warning in the UI if possible or just log it.
            const manualContainer = this.container.querySelector('.manual-editor-container');
            if (manualContainer && !manualContainer.classList.contains('hidden')) {
                // Already in manual, just log it.
            }

            return false;
        }
    }

    validateVisualConfig() {
        let valid = true;
        const s = this.state.security;
        const n = this.state.network;

        const require = (selector, msg) => {
            const el = this.container.querySelector(selector);
            if (el && !el.value && !el.closest('.hidden')) {
                el.classList.add('border-red-500');
                valid = false;
            } else if (el) {
                el.classList.remove('border-red-500');
            }
        };

        if (s === 'reality') {
            require('.vis-reality-dest', 'Dest is required');
            require('.vis-reality-sni', 'SNI is required');
        }

        return valid;
    }

    // Helpers
    getVal(selector, fallback) {
        const el = this.container.querySelector(selector);
        return el ? el.value : fallback;
    }
    setVal(selector, val) {
        const el = this.container.querySelector(selector);
        if (el && val !== undefined) el.value = val;
    }
}

// Global helper for simple non-instantiated use or legacy calls
window.VisualEditor = VisualEditor;
