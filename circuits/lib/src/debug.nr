// Circuit-level debugging utilities (compile-time controlled)
//
// All debug output is removed in release builds via dead code elimination.
// Enable with: nargo build --debug
// Disable with: nargo build --release (default)

pub global DEBUG_ENABLED: bool = false;

// Debug output for a single field value (name-value pair)
pub fn debug_field(_label: str<32>, _value: Field) {
    if DEBUG_ENABLED {
    }
}

// Debug output for a single u32 value
pub fn debug_u32(_label: str<32>, _value: u32) {
    if DEBUG_ENABLED {
    }
}

// Debug output for a card value with human-readable format
// Card encoding: suit * 13 + rank
// suit: 0=Clubs, 1=Diamonds, 2=Hearts, 3=Spades
// rank: 0=2, 1=3, ..., 8=10, 9=J, 10=Q, 11=K, 12=A
pub fn debug_card(_card: Field) {
    if DEBUG_ENABLED {
    }
}

// Debug output for array of fields
pub fn debug_array_fields<let N: u32>(_label: str<32>, _arr: [Field; N]) {
    if DEBUG_ENABLED {
    }
}

// Debug output for array of u32 values
pub fn debug_array_u32<let N: u32>(_label: str<32>, _arr: [u32; N]) {
    if DEBUG_ENABLED {
    }
}

// Debug output for assertion results (helps identify which assertion failed)
pub fn debug_assertion(_condition: bool, _label: str<32>) {
    if DEBUG_ENABLED {
    }
}
