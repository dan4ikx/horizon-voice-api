# Realtime audio and mod networking

The WebAssembly runtime, SDK and voice-lab examples have been removed with the upstream native-mod migration. Their WIT interfaces are no longer available.

Use [native voice services](../native-voice.md) on native hook API 4 for microphone PCM, positional playback, UDP/DNS and binary custom payloads. See the [permission design](../native-voice-permissions.md) and [native example bundle](../../examples/native-voice-lab/).

Native hook API 3 remains the upstream world lifecycle contract. API 4 adds the voice services without changing those lifecycle hooks.
