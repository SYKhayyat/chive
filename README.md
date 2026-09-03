# chive

A command-line tool that scans your file system, builds a searchable database of file categories and metadata, identifies recoverable files, and enables file recovery.

## Overview

chive crawls your filesystem, extracts metadata, categorizes files, detects deleted entries (via MFT/inode parsing), and stores everything in a SQLite database. You can then search, filter, and recover files from the database.

## Features

- **File System Scanner**: Crawls directories, extracts metadata (size, dates, permissions, type)
- **File Categorization**: Automatically categorizes files (documents, images, code, configs, etc.)
- **Deleted File Detection**: Reads MFT (NTFS) / inode (ext4) to find deleted but recoverable files
- **Metadata Database**: SQLite database storing file catalog with categories, tags, recovery status
- **Recovery Engine**: Block-level recovery of deleted files with integrity verification
- **CLI Interface**: Search, filter, and recover commands with various options
- **Analytics**: File distribution statistics, recovery recommendations

## Usage

```bash
# Scan your home directory
chive scan ~/ 

# Search for documents modified in the last 30 days
chive search --type document --modified "30d"

# Find large recoverable files
chive search --recoverable --size ">100MB"

# Recover a specific file
chive recover <file-id> --output ~/recovery/

# View database statistics
chive stats
```

## Language Options

| Language | Familiarity | Performance | Dev Speed | CLI Ecosystem | Risk |
|----------|-------------|-------------|-----------|---------------|------|
| Python | Well-known | Slower | Fastest | Excellent | Low |
| Java | Expert | Good | Moderate | Good | Low |
| Go | None | Good | Fast | Excellent | Medium |
| Rust | Basic | Best | Slow | Excellent | High |

## Recommendation

**Python** for fastest shipping. You know it well, AI fills gaps, focus on product quality.

**Java** for zero risk. You're experts. Verbose but reliable.

**Go** for learning experience. Best balance of performance and productivity, but timeline is tight.

## Getting Started

```bash
# Clone the repository
git clone https://github.com/<your-username>/chive.git
cd chive

# Install dependencies (Python example)
pip install -r requirements.txt

# Run a scan
chive scan ~/Documents
```

## Project Structure

```
chive/
├── README.md
├── docs/
│   └── SUMMARY.md    # Detailed language comparison
├── src/
│   └── chive/
│       ├── __init__.py
│       ├── cli.py        # CLI interface
│       ├── scanner.py    # File system scanner
│       ├── database.py   # SQLite database layer
│       ├── recovery.py   # File recovery engine
│       └── models.py     # Data models
├── tests/
│   └── ...
└── requirements.txt
```

## License

No license - all rights reserved.
