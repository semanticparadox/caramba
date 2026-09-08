// Self-contained generator for the CSM/1 test-vector corpus.
// Standard library only, by requirement: the corpus must not inherit a bug
// from a third-party CBOR, Ed25519 or HPKE implementation that one of the
// three target languages does not share.
module csm1vectors

go 1.26
