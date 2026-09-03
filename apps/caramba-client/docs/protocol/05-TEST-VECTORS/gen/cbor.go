package main

// A strict CBOR encoder for the CSM/1 profile of 03-WIRE.md section 3.
//
// This encoder is deliberately unable to emit anything the profile forbids.
// It has no indefinite-length mode, no tag support, no float support, no
// negative integers, no null, no text or byte string map keys, and it panics
// on a duplicate or out-of-range map key rather than emitting one. That
// inability is the point: 03-WIRE.md section 3 claims the profile is
// unambiguous, and a from-scratch encoder that can express every legal
// document and no illegal one is the existence proof of that claim.
//
// Malformed fixtures are NOT produced by weakening this encoder. They are
// produced by splicing bytes into an encoder output (see negative.go), so
// there is exactly one code path that emits conforming bytes.

import (
	"fmt"
	"sort"
	"unicode/utf8"
)

// Profile limits, 03-WIRE.md section 3.2.
const (
	maxDepth      = 6
	maxMapPairs   = 64
	maxArrayItems = 512
	maxTstrBytes  = 256
	maxBstrBytes  = 3072
	maxUint       = uint64(1)<<53 - 1
)

// Key ranges, 03-WIRE.md section 3.3.
const (
	criticalKeyMin    = 1
	criticalKeyMax    = 63
	nonCriticalKeyMin = 64
	nonCriticalKeyMax = 1023
)

type val interface{ enc(depth int) []byte }

// u is a CBOR unsigned integer, major type 0.
type u uint64

// b is a CBOR byte string, major type 2.
type b []byte

// t is a CBOR text string, major type 3.
type t string

// boolean is a CBOR simple value, 0xf4 or 0xf5. The profile permits no other.
type boolean bool

// arr is a CBOR array, major type 4.
type arr []val

// cmap is a CBOR map, major type 5, with unsigned integer keys only.
// Insertion order is irrelevant: enc sorts, so a builder cannot accidentally
// emit an out-of-order map (rule C10).
type cmap struct {
	keys []uint64
	vals map[uint64]val
}

func m() *cmap { return &cmap{vals: map[uint64]val{}} }

// set adds a pair. It panics on a duplicate key or a key outside the legal
// ranges, because both are encoder bugs, not fixture content.
func (c *cmap) set(k uint64, v val) *cmap {
	if _, dup := c.vals[k]; dup {
		panic(fmt.Sprintf("cbor: duplicate map key %d", k))
	}
	if k < criticalKeyMin || k > nonCriticalKeyMax {
		panic(fmt.Sprintf("cbor: map key %d outside 1..1023", k))
	}
	c.keys = append(c.keys, k)
	c.vals[k] = v
	return c
}

// setIf adds a pair only when cond holds. Used for optional fields so the
// builders read as field tables rather than as branching code.
func (c *cmap) setIf(cond bool, k uint64, v val) *cmap {
	if cond {
		c.set(k, v)
	}
	return c
}

func (c *cmap) has(k uint64) bool { _, ok := c.vals[k]; return ok }

// head emits the shortest-form head for a major type and argument (rule C4).
func head(major byte, arg uint64) []byte {
	mt := major << 5
	switch {
	case arg < 24:
		return []byte{mt | byte(arg)}
	case arg <= 0xff:
		return []byte{mt | 24, byte(arg)}
	case arg <= 0xffff:
		return []byte{mt | 25, byte(arg >> 8), byte(arg)}
	case arg <= 0xffffffff:
		return []byte{mt | 26, byte(arg >> 24), byte(arg >> 16), byte(arg >> 8), byte(arg)}
	default:
		return []byte{mt | 27,
			byte(arg >> 56), byte(arg >> 48), byte(arg >> 40), byte(arg >> 32),
			byte(arg >> 24), byte(arg >> 16), byte(arg >> 8), byte(arg)}
	}
}

func (v u) enc(depth int) []byte {
	if uint64(v) > maxUint {
		panic(fmt.Sprintf("cbor: uint %d exceeds MAX_UINT", uint64(v)))
	}
	return head(0, uint64(v))
}

func (v b) enc(depth int) []byte {
	if len(v) > maxBstrBytes {
		panic(fmt.Sprintf("cbor: bstr of %d bytes exceeds MAX_BSTR_BYTES", len(v)))
	}
	return append(head(2, uint64(len(v))), v...)
}

func (v t) enc(depth int) []byte {
	if len(v) > maxTstrBytes {
		panic(fmt.Sprintf("cbor: tstr of %d bytes exceeds MAX_TSTR_BYTES", len(v)))
	}
	if !utf8.ValidString(string(v)) {
		panic("cbor: tstr is not well-formed UTF-8 (rule C11)")
	}
	return append(head(3, uint64(len(v))), v...)
}

func (v boolean) enc(depth int) []byte {
	if v {
		return []byte{0xf5}
	}
	return []byte{0xf4}
}

func (v arr) enc(depth int) []byte {
	if depth+1 > maxDepth {
		panic("cbor: nesting exceeds MAX_DEPTH")
	}
	if len(v) > maxArrayItems {
		panic(fmt.Sprintf("cbor: array of %d items exceeds MAX_ARRAY_ITEMS", len(v)))
	}
	out := head(4, uint64(len(v)))
	for _, item := range v {
		out = append(out, item.enc(depth+1)...)
	}
	return out
}

func (c *cmap) enc(depth int) []byte {
	if depth+1 > maxDepth {
		panic("cbor: nesting exceeds MAX_DEPTH")
	}
	if len(c.keys) > maxMapPairs {
		panic(fmt.Sprintf("cbor: map of %d pairs exceeds MAX_MAP_PAIRS", len(c.keys)))
	}
	ks := append([]uint64(nil), c.keys...)
	sort.Slice(ks, func(i, j int) bool { return ks[i] < ks[j] })
	out := head(5, uint64(len(ks)))
	for _, k := range ks {
		out = append(out, head(0, k)...)
		out = append(out, c.vals[k].enc(depth+1)...)
	}
	return out
}

// encode renders a top-level map. Rule C1 requires the payload to be exactly
// one top-level CBOR map, so this is the only entry point.
func encode(c *cmap) []byte { return c.enc(0) }

// raw is an escape hatch used only by negative.go to place hand-built,
// deliberately illegal bytes where a value would go. It never appears in a
// positive fixture.
type raw []byte

func (v raw) enc(depth int) []byte { return []byte(v) }

// bstrOf builds the encoded form of a byte string of n zero bytes, which is
// what the padding field pd needs (03-WIRE.md section 12.2).
func padField(n int) []byte { return b(make([]byte, n)).enc(1) }

// padCost is the encoded cost in bytes of a pd field carrying n zero bytes,
// including its 1-byte key head. 03-WIRE.md section 12.2.
func padCost(n int) int {
	switch {
	case n < 24:
		return 2 + n
	case n <= 255:
		return 3 + n
	default:
		return 4 + n
	}
}

// padBytesFor returns the number of zero bytes N such that the encoded pd
// field costs exactly d bytes, and reports whether d is reachable.
// 03-WIRE.md section 12.2 enumerates 1, 26 and 259 as unreachable.
func padBytesFor(d int) (int, bool) {
	switch {
	case d == 0:
		return 0, true // pd omitted entirely
	case d >= 2 && d <= 25:
		return d - 2, true
	case d >= 27 && d <= 258:
		return d - 3, true
	case d >= 260 && d <= maxBstrBytes+4:
		return d - 4, true
	default:
		return 0, false
	}
}
