# Beancount Language Server Brownfield Enhancement PRD

## Intro Project Analysis and Context

### SCOPE ASSESSMENT

This PRD addresses SIGNIFICANT enhancements to the existing beancount language server that require comprehensive planning and multiple coordinated stories. The project already has a mature, production-ready foundation that supports substantial architectural improvements.

### Existing Project Overview

**Analysis Source:** Document-project output available at `docs/brownfield-architecture.md`

**Current Project State:** 
Production-ready Language Server Protocol (LSP) implementation written in Rust that provides rich editing features for Beancount accounting files. The system includes:

- Multi-method validation system (system call, Python script, PyO3 embedded)
- Complete LSP feature set (completions, diagnostics, formatting, references, rename)
- VSCode extension with marketplace publication
- Cross-platform deployment (Linux, macOS, Windows)
- Nix-based development environment
- Comprehensive CI/CD pipeline

### Available Documentation Analysis

✅ **Using existing project analysis from document-project output**

Key documents available from document-project:
- ✅ Tech Stack Documentation (comprehensive)
- ✅ Source Tree/Architecture (detailed module breakdown)
- ✅ API Documentation (LSP protocol implementation)
- ✅ External API Documentation (beancount Python integration)
- ✅ Technical Debt Documentation (identified constraints and improvements)
- ✅ Performance Characteristics (current benchmarks)
- ✅ Development Workflow (complete setup instructions)

### Enhancement Scope Definition

**Enhancement Type:** 
- ✅ New Feature Addition
- ✅ Major Feature Modification
- ✅ Performance/Scalability Improvements

**Enhancement Description:** 
Expand the beancount language server with additional LSP capabilities (hover, go-to-definition, document symbols, etc.), enhance existing features (completions, diagnostics, formatting), and significantly improve diagnostic performance through optimization and potentially new validation strategies.

**Impact Assessment:** 
- ✅ Significant Impact (substantial existing code changes)
- ✅ Major Impact (architectural changes required for performance improvements)

### Goals and Background Context

**Goals:**
- Implement missing LSP features to provide comprehensive IDE experience
- Enhance existing LSP features with better accuracy and user experience
- Dramatically improve diagnostic performance for better real-time feedback
- Maintain backward compatibility and existing feature quality
- Leverage existing clean architecture for sustainable expansion

**Background Context:**
The beancount language server has established itself as a production-ready LSP implementation with solid fundamentals. However, users need more comprehensive IDE features to match modern development experiences. Current diagnostic performance, while functional, could be significantly improved for larger projects. The existing pluggable architecture and clean codebase provide an excellent foundation for these enhancements without compromising stability.

**Change Log:**
| Change | Date | Version | Description | Author |
|--------|------|---------|-------------|--------|
| Initial PRD Creation | 2025-01-09 | 1.0 | Comprehensive LSP enhancement planning | PM John |

## Requirements

### Functional

**FR1:** The language server shall implement hover support showing account balances, transaction details, and metadata when hovering over beancount elements

**FR2:** The language server shall provide go-to-definition functionality for navigating to account/payee/commodity definitions across files

**FR3:** The language server shall implement document symbols support for outline views showing accounts, transactions, and file structure

**FR4:** The language server shall add folding ranges capability for collapsing transactions, account hierarchies, and multi-line entries

**FR5:** The language server shall implement semantic highlighting providing enhanced syntax coloring with semantic information

**FR6:** The existing completion system shall be enhanced with better context awareness and additional completion types (commodities, tags, links)

**FR7:** The existing formatting system shall support additional configuration options and improved algorithm efficiency

**FR8:** The diagnostics system shall implement performance optimizations including caching, incremental validation, and parallel processing

**FR9:** The language server shall add a new high-performance validation method optimized for real-time diagnostics

**FR10:** All new features shall integrate seamlessly with the existing tree-sitter parsing and document forest management

### Non Functional

**NFR1:** Enhancement must maintain existing performance characteristics for current features and not exceed memory usage by more than 30%

**NFR2:** New diagnostic optimizations must achieve at least 50% performance improvement for projects with 20+ files

**NFR3:** All new LSP features must respond within 200ms for typical beancount files (up to 1000 lines)

**NFR4:** The enhanced system must maintain 99.9% backward compatibility with existing editor configurations

**NFR5:** Code quality standards must be maintained with comprehensive test coverage (>80%) for all new features

**NFR6:** New features must support all existing platforms (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64)

### Compatibility Requirements

**CR1:** All existing LSP protocol implementations must remain fully functional without configuration changes

**CR2:** The current three-method validation system (system call, Python script, PyO3 embedded) must be preserved and enhanced

**CR3:** VSCode extension and all documented editor integrations must continue working without modification

**CR4:** Existing configuration schema must remain valid with new options being additive only

## Technical Constraints and Integration Requirements

### Existing Technology Stack

**Languages**: Rust 1.75.0+ (edition 2021), Python 3.x (for beancount integration), TypeScript 4.6.3 (VSCode extension)

**Frameworks**: LSP Server 0.7 + LSP Types 0.97 (LSP protocol), Tree-sitter 2.4.1 (parsing), PyO3 0.25 (optional Python embedding)

**Database**: File-based (beancount plain text files with include resolution)

**Infrastructure**: Cross-platform binary distribution via cargo-dist, Nix flake development environment, GitHub Actions CI/CD

**External Dependencies**: Beancount Python library (required), bean-check binary (optional), tree-sitter-beancount grammar

### Integration Approach

**Database Integration Strategy**: Enhance existing file-based document forest management with caching layers for computed data (balances, symbol tables). Maintain compatibility with beancount include file resolution.

**API Integration Strategy**: Extend existing LSP protocol handlers with new capabilities while preserving current request/response patterns. Add new providers following established provider pattern in `src/providers/`.

**Frontend Integration Strategy**: Leverage existing tree-sitter parsing infrastructure for new semantic analysis. Enhance existing completion and diagnostic providers with optimized algorithms and caching.

**Testing Integration Strategy**: Extend existing insta snapshot testing for new features. Add performance benchmarks for diagnostic improvements. Maintain existing test coverage standards.

### Code Organization and Standards

**File Structure Approach**: Follow established provider pattern - new LSP features as modules in `src/providers/`, shared utilities in existing utility modules. Performance optimizations integrated into existing checker system.

**Naming Conventions**: Maintain existing Rust naming conventions and LSP protocol naming. New diagnostic methods follow established `BeancountChecker` trait pattern.

**Coding Standards**: Adhere to existing cargo fmt + clippy standards. Maintain existing error handling patterns with anyhow/thiserror. Continue structured logging with tracing framework.

**Documentation Standards**: Follow existing inline documentation style. Update README.md feature tables. Add performance benchmarks to documentation.

### Deployment and Operations

**Build Process Integration**: Utilize existing cargo-dist cross-platform builds. Maintain optional PyO3 feature compilation. Ensure new features work in Nix development environment.

**Deployment Strategy**: Leverage existing release automation via GitHub Actions. Maintain backward compatibility for existing installations. Update VSCode extension as needed for new features.

**Monitoring and Logging**: Enhance existing tracing-based logging with performance metrics. Add diagnostic timing information. Maintain existing log level configurability.

**Configuration Management**: Extend existing JSON-based LSP configuration schema additively. Maintain existing default values and backward compatibility.

### Risk Assessment and Mitigation

**Technical Risks**: 
- Performance optimizations may introduce complexity that affects maintainability
- New LSP features could impact single-threaded processing limitations noted in technical debt
- Tree-sitter parsing overhead for semantic analysis features may affect responsiveness

**Integration Risks**:
- Changes to diagnostic system could affect existing three-method validation strategy
- New caching layers may introduce state management complexity
- Enhanced features may stress existing document forest management under high file counts

**Deployment Risks**:
- Additional dependencies or optional features may complicate cross-platform builds
- Performance optimizations may behave differently across target platforms
- VSCode extension updates may require marketplace approval delays

**Mitigation Strategies**:
- Implement performance features behind feature flags for gradual rollout
- Maintain existing validation methods while adding optimized variants
- Use comprehensive benchmarking to validate performance improvements across platforms
- Design caching as optional enhancement that degrades gracefully
- Implement thorough integration testing with existing editor configurations

## Epic and Story Structure

### Epic Approach

**Epic Structure Decision**: Single comprehensive epic with rationale: Maintains architectural coherence while enabling systematic development that builds optimizations first, then features, ensuring each story benefits from previous improvements.

## Epic 1: Comprehensive LSP Enhancement

**Epic Goal**: Transform the beancount language server into a comprehensive, high-performance IDE experience by implementing missing LSP features, optimizing diagnostic performance, and enhancing existing capabilities while maintaining full backward compatibility.

**Integration Requirements**: All enhancements must integrate seamlessly with existing tree-sitter parsing, document forest management, and three-method validation architecture. Performance improvements must be measurable and not compromise existing functionality.

### Story 1.1: Diagnostic Performance Foundation

As a **beancount developer**,  
I want **significantly faster diagnostic feedback with caching and optimization**,  
so that **I can work efficiently with larger beancount projects without waiting for validation**.

#### Acceptance Criteria

1. **Diagnostic caching system**: Implement result caching that invalidates appropriately on file changes
2. **Incremental validation**: Only re-validate changed files and their dependencies  
3. **Performance benchmarks**: Achieve 50%+ performance improvement for projects with 20+ files
4. **Memory efficiency**: Caching system uses <30% additional memory overhead
5. **Configurable optimization**: Users can enable/disable optimization features via configuration

#### Integration Verification

**IV1**: All existing validation methods (system call, Python script, PyO3 embedded) continue to function correctly with caching layer
**IV2**: Existing diagnostic accuracy is maintained - no false positives or missed errors introduced  
**IV3**: VSCode extension and all editor integrations continue to receive diagnostics without modification

### Story 1.2: Enhanced Symbol Analysis Infrastructure  

As a **beancount developer**,
I want **improved symbol extraction and analysis for accounts, payees, and commodities**,
so that **new LSP features have accurate data to work with**.

#### Acceptance Criteria

1. **Symbol database**: Comprehensive extraction of accounts, payees, commodities, tags, and links with metadata
2. **Cross-file resolution**: Symbol references resolved across included files using existing forest management
3. **Incremental updates**: Symbol database updates efficiently when files change
4. **Memory efficiency**: Symbol data structures optimized for lookup performance
5. **API consistency**: Symbol data accessible through consistent internal APIs

#### Integration Verification

**IV1**: Existing completion system continues to work and benefits from enhanced symbol data
**IV2**: Document forest management integrates seamlessly with new symbol extraction
**IV3**: Tree-sitter parsing performance is not significantly impacted by additional analysis

### Story 1.3: Hover Information Support

As a **beancount user**,  
I want **informative hover tooltips showing account details, transaction context, and computed information**,  
so that **I can understand my beancount data without navigating away from my current location**.

#### Acceptance Criteria

1. **Account hover**: Show account metadata, recent transactions, and balance information when available
2. **Transaction hover**: Display transaction details, posting breakdowns, and validation status
3. **Commodity hover**: Show commodity definitions, price information, and usage statistics  
4. **Performance target**: Hover responses delivered within 200ms for typical files
5. **Graceful degradation**: Hover works with partial information when full computation unavailable

#### Integration Verification

**IV1**: Hover implementation doesn't interfere with existing LSP request handling
**IV2**: Tree-sitter parsing provides adequate AST information for hover positioning
**IV3**: Existing diagnostic and completion systems continue to operate normally

### Story 1.4: Go-to-Definition and References Enhancement

As a **beancount developer**,  
I want **precise navigation to symbol definitions and comprehensive reference finding**,  
so that **I can efficiently explore and understand large beancount codebases**.

#### Acceptance Criteria

1. **Go-to-definition**: Navigate to account opens, payee first usage, and commodity definitions
2. **Enhanced references**: Find all references with context information and usage type
3. **Cross-file navigation**: Seamless navigation across included files using forest management
4. **Symbol disambiguation**: Handle cases where symbols might have multiple definitions
5. **Performance optimization**: Leverage symbol database for fast lookups

#### Integration Verification  

**IV1**: Existing reference finding functionality is enhanced, not replaced
**IV2**: Document forest includes and dependencies are properly traversed
**IV3**: Navigation works correctly with existing editor configurations

### Story 1.5: Document Symbols and Structure

As a **beancount developer**,  
I want **hierarchical document outline showing accounts, transactions, and file structure**,  
so that **I can quickly navigate and understand the organization of my beancount files**.

#### Acceptance Criteria

1. **Hierarchical symbols**: Account hierarchies displayed as nested structures  
2. **Transaction grouping**: Transactions grouped by date, type, or other logical criteria
3. **Symbol metadata**: Include line numbers, types, and additional context information
4. **Performance efficiency**: Symbol extraction integrated with existing parsing pipeline
5. **Filtering support**: Support for filtering symbols by type or other criteria

#### Integration Verification

**IV1**: Document symbol extraction doesn't impact existing tree-sitter parsing performance  
**IV2**: Symbol information remains accurate as files are edited
**IV3**: Multi-file projects show symbols appropriately across file boundaries

### Story 1.6: Enhanced Completion System

As a **beancount developer**,  
I want **smarter completions with better context awareness and additional completion types**,  
so that **I can write beancount code more efficiently with fewer errors**.

#### Acceptance Criteria

1. **Context-aware completions**: Account suggestions filtered by transaction context and patterns
2. **Commodity completion**: Complete commodity names with proper formatting  
3. **Enhanced payee completion**: Payee suggestions with recent usage prioritization
4. **Tag and link completion**: Complete hashtags and links with validation
5. **Performance improvement**: Completion responses within 100ms using symbol database

#### Integration Verification

**IV1**: Existing completion functionality is enhanced while maintaining backward compatibility
**IV2**: Completion accuracy improves without introducing incorrect suggestions
**IV3**: VSCode extension and editor integrations receive improved completions seamlessly

### Story 1.7: Folding Ranges and Semantic Highlighting  

As a **beancount developer**,  
I want **code folding for transactions and semantic highlighting for better code readability**,  
so that **I can manage large beancount files more effectively and understand code structure visually**.

#### Acceptance Criteria

1. **Transaction folding**: Fold individual transactions and transaction blocks
2. **Account hierarchy folding**: Collapse account sections and related entries
3. **Semantic highlighting**: Enhanced syntax coloring based on semantic analysis
4. **Performance efficiency**: Folding and highlighting computed efficiently during parsing  
5. **Editor compatibility**: Features work across LSP-compatible editors

#### Integration Verification

**IV1**: Folding ranges don't interfere with existing text editing and formatting
**IV2**: Semantic highlighting enhances but doesn't conflict with existing syntax highlighting
**IV3**: Tree-sitter parsing pipeline accommodates additional analysis without performance degradation