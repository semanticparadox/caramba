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

        // 2. Hook up Visual Controls
        // Protocol is always VLESS now, so no listener needed for it
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
        const form = this.container.closest('form') || this.container.querySelector('form');
        if (form) {
            form.addEventListener('submit', (e) => {
                if (this.state.mode === 'visual') {
                    if (!this.validateVisualConfig()) {
                        e.preventDefault();
                        e.stopPropagation();
                    } else {
                        this.generateJsonFromVisual();
                    }
                }
            });
        }

        // 5. Initial State
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
            this.parseJsonToVisual();
        } else {
            visualContainer?.classList.add('hidden');
            manualContainer?.classList.remove('hidden');
            this.generateJsonFromVisual();
        }
    }

    updateVisualState() {
        // Always VLESS
        this.state.protocol = 'vless';

        const nEl = this.container.querySelector('.vis-network');
        const sEl = this.container.querySelector('.vis-security');

        if (!nEl || !sEl) return;

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

        // Always show client section for VLESS
        toggle('.client-section', true);

        // Flow is only for VLESS + Reality + TCP
        toggle('.flow-section', p === 'vless' && s === 'reality' && n === 'tcp');

        // Transports
        this.container.querySelectorAll('.transport-settings').forEach(el => el.classList.add('hidden'));
        const transportMap = {
            'tcp': '.tcp-settings',
            'ws': '.ws-settings',
            'grpc': '.grpc-settings',
            'httpupgrade': '.httpupgrade-settings',
            'quic': '.quic-settings',
            'xhttp': '.xhttp-settings' // Added XHTTP support just in case, though template might not have it yet
        };
        if (transportMap[n]) toggle(transportMap[n], true);

        // Security
        toggle('.tls-settings', s === 'tls');
        toggle('.reality-settings', s === 'reality');

        if (this.state.mode === 'visual') {
            this.generateJsonFromVisual();
        }
    }

    generateJsonFromVisual() {
        if (this.state.mode === 'manual') return;

        // Force VLESS
        const protocol = 'vless';
        const network = this.container.querySelector('.vis-network').value;
        const security = this.container.querySelector('.vis-security').value;

        // --- Settings JSON ---
        let settings = {
            protocol: protocol,
            clients: [],
            decryption: "none",
            fallbacks: []
        };

        // Add default client placeholder
        settings.clients.push({
            id: "{{uuid}}",
            flow: (protocol === 'vless' && security === 'reality' && network === 'tcp') ? "xtls-rprx-vision" : undefined
        });

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
                privateKey: "{{reality_private}}",
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

            // Protocol - Ignored (Always VLESS)
            // if (settings.protocol) { ... }

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
