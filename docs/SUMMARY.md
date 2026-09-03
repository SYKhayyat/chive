# chive - Language Analysis & Project Summary

## Project Overview

**chive** is a file system scanner and recovery CLI tool that:
1. Scans your laptop's file system
2. Builds a searchable database of file categories and metadata
3. Identifies recoverable files (deleted but still on disk)
4. Enables file recovery from the database

### Core Components

1. **Scanner Engine** - Crawls filesystem, extracts metadata, categorizes files, detects deleted entries (MFT/inode parsing)
2. **Metadata Database** - SQLite storing file catalog with categories, tags, recovery status
3. **Recovery Engine** - Reads deleted file structures, attempts block-level recovery, verifies integrity
4. **CLI Interface** - Searchable commands with filters (type, date, size, recoverability)

### Enterprise Requirements

- **Multi-tiered**: CLI frontend + Scanner/Recovery backend + Database layer
- **Componentized**: Scanner, Database, Recovery, CLI are separate modules
- **Cloud**: Can sync database to cloud for cross-device access
- **Concurrency**: Parallel file scanning, thread-safe database operations
- **Security**: Encrypted database option, permission handling, audit logs

---

## Language Options & Trade-offs

### 1. Python

**Your Familiarity**: Well-known

**Pros**:
- Fastest development speed
- Excellent CLI libraries (typer, click)
- Built-in sqlite3 module
- AI assistance fills gaps easily
- Great for prototyping and rapid iteration

**Cons**:
- Slower performance on large drives
- GIL limits true parallelism
- Packaging/distribution can be tricky
- Not ideal for low-level file system operations

**Best For**: Shipping fast, focusing on product quality over language learning

**Risk Level**: Low

---

### 2. Java

**Your Familiarity**: Expert

**Pros**:
- You're experts - can understand, modify, and explain all code
- Rock-solid stability
- Good CLI library (Picocli)
- jsqlite for database operations
- No surprises - familiar territory

**Cons**:
- Verbose - more boilerplate code
- Not ideal for CLI tools
- Slower startup time
- More lines of code to maintain

**Best For**: Leveraging existing expertise, ensuring full code understanding

**Risk Level**: Low

---

### 3. Go

**Your Familiarity**: None

**Pros**:
- Great concurrency with goroutines
- Excellent CLI framework (Cobra)
- Cross-compilation built-in
- Good performance
- Clean, readable syntax
- Strong standard library

**Cons**:
- You don't know it
- 12-14 weeks is tight for learning + building
- Moderate learning curve
- Need to understand goroutines, channels, error handling

**Best For**: Learning something new while shipping a solid product

**Risk Level**: Medium

---

### 4. Rust

**Your Familiarity**: Basic

**Pros**:
- Best performance
- Memory safety without garbage collector
- Impressive on resume
- Excellent CLI framework (Clap)
- Great for low-level file system work

**Cons**:
- Steepest learning curve
- Most complex syntax
- Highest risk for semester timeline
- Ownership/borrowing concepts take time to master

**Best For**: Maximum technical impressiveness if time permits

**Risk Level**: High

---

### 5. C

**Your Familiarity**: Basic

**Pros**:
- Maximum performance
- Direct file system access
- Industry standard for systems programming

**Cons**:
- Memory management headaches
- Safety concerns (buffer overflows, segfaults)
- Slow development
- Not recommended for semester project

**Best For**: Not recommended

**Risk Level**: High

---

### 6. C++

**Your Familiarity**: Basic

**Pros**:
- Good performance
- Familiar to team (basic)
- Powerful abstractions

**Cons**:
- Complex build systems
- Not ideal for CLI tools
- Compile times can be long
- Memory management concerns

**Best For**: Not recommended

**Risk Level**: High

---

### 7. TypeScript/Node.js

**Your Familiarity**: Unknown

**Pros**:
- Good CLI libraries (commander)
- Async I/O
- JSON handling

**Cons**:
- Not ideal for low-level file operations
- Performance concerns
- Not suitable for file system recovery

**Best For**: Not recommended for this project

**Risk Level**: Medium

---

### 8. Kotlin

**Your Familiarity**: Unknown

**Pros**:
- Modern JVM language
- Concise syntax
- Good CLI libraries
- Interoperable with Java

**Cons**:
- Additional learning
- JVM overhead
- Smaller ecosystem than Java

**Best For**: If team wants something modern but JVM-based

**Risk Level**: Medium

---

### 9. C#

**Your Familiarity**: Unknown

**Pros**:
- Good libraries
- Cross-platform with .NET Core
- Enterprise features

**Cons**:
- Not ideal for CLI
- Verbose
- Smaller CLI ecosystem

**Best For**: Not recommended

**Risk Level**: Medium

---

### 10. Zig

**Your Familiarity**: None

**Pros**:
- Modern systems language
- Good performance
- Growing ecosystem

**Cons**:
- Very new
- Limited ecosystem
- Steep learning curve
- Small community

**Best For**: Not recommended for semester project

**Risk Level**: High

---

## Comparison Table

| Language | Familiarity | Performance | Dev Speed | CLI Ecosystem | Learning Curve | Risk Level |
|----------|-------------|-------------|-----------|---------------|----------------|------------|
| Python | Well-known | Slower | Fastest | Excellent | None | Low |
| Java | Expert | Good | Moderate | Good | None | Low |
| Go | None | Good | Fast | Excellent | Moderate | Medium |
| Rust | Basic | Best | Slow | Excellent | Steep | High |
| C | Basic | Best | Slow | Basic | Steep | High |
| C++ | Basic | Best | Slow | Moderate | Steep | High |
| TypeScript | Unknown | Moderate | Fast | Good | Low | Medium |
| Kotlin | Unknown | Good | Moderate | Good | Low | Medium |
| C# | Unknown | Good | Moderate | Good | Low | Medium |
| Zig | None | Best | Slow | Basic | Steep | High |

---

## Recommendation

Given your Java/Python skills, 12-14 week timeline, and need to understand all code:

### Primary: Python
- Fastest to ship
- You know it well
- AI fills gaps easily
- Focus on product quality

### Alternative: Java
- Zero risk
- You're experts
- Verbose but reliable
- No surprises

### Wildcard: Go
- Best balance of performance and productivity
- Learning experience
- Timeline is tight but feasible with AI help

---

## Project Timeline

### Week 1-2: Setup & Planning
- Choose language
- Define MVP features
- Set up project structure
- Design database schema

### Week 3-5: Core Scanner
- File system crawler
- Metadata extraction
- File categorization
- Deleted file detection

### Week 6-8: Database & Search
- SQLite integration
- Search functionality
- Query optimization

### Week 9-11: Recovery Engine
- Block-level recovery
- Integrity verification
- Error handling

### Week 12-14: Polish & Demo
- CLI refinement
- Documentation
- Testing
- Demo preparation

---

## Technical Challenges

1. **File System Parsing**: Different file systems (NTFS, ext4, APFS) have different structures
2. **Deleted File Detection**: Reading MFT/inode tables requires low-level access
3. **Performance**: Scanning large drives efficiently
4. **Database Design**: Schema for complex queries and indexing
5. **Recovery Algorithms**: Verifying file integrity after recovery
6. **Cross-Platform**: Supporting Windows, Linux, macOS

---

## Next Steps

1. Choose language
2. Create project structure
3. Start with file system scanner module
4. Build database layer
5. Implement CLI interface
6. Add recovery engine
7. Test with real scenarios
8. Prepare demo
