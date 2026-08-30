import os
import re
import glob

src_dir = '/home/peterjune/StellPoker/contracts/poker-table/src'

for filename in glob.glob(os.path.join(src_dir, '*.rs')):
    if os.path.basename(filename) in ['types.rs', 'game.rs', 'betting.rs', 'update_fixtures.py']:
        continue

    with open(filename, 'r') as f:
        content = f.read()

    original = content
    
    content = re.sub(r'(max_buy_in:\s*[^,]+,)(\s*)(?!betting_structure)', r'\1\2betting_structure: crate::types::BettingStructure::NoLimit,\2', content)
    content = re.sub(r'(ante:\s*)0(\s*,)', r'\1crate::types::AnteMode::None\2', content)
    content = re.sub(r'(ante:\s*)([0-9]+)(\s*,)', r'\1crate::types::AnteMode::Fixed(\2)\3', content)
    
    content = re.sub(r'(break_ends_at:\s*[^,]+,)(\s*)(?!settlement_entered_ledger)', r'\1\2settlement_entered_ledger: 0,\2', content)

    if original != content:
        with open(filename, 'w') as f:
            f.write(content)
        print(f"Updated {filename}")
