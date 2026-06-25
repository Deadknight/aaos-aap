# v21b

Fixes the first ServiceDiscoveryRequest proto build by removing the unsupported
`set_phone_info` call. Next step: confirm DHU receives the encrypted
ServiceDiscoveryRequest; then fill optional phone_info through the generated API
if needed.
