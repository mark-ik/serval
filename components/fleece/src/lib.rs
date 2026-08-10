//! Name reservation for **fleece**, live document extraction for the Genet
//! engine.
//!
//! To fleece a document is to shear the readable substance off a rendered
//! page: article text, metadata, tables, and structure come away clean, and
//! the document keeps standing. This is the successor name for
//! `genet-extract`'s lane: render-free content extraction over the
//! profile-neutral LayoutDom. Analyze, don't paint.
//!
//! The boundaries are the point:
//!
//! - **Not import.** Mere's `import` migrates *stored* browser data
//!   (bookmarks, history); fleece works a *live* document.
//! - **Not crawl.** The frontier decides what to visit; fleece decides what
//!   a visited page said.
//! - **Not illume.** The lexer names spans in source text; fleece harvests
//!   rendered documents.
//!
//! No implementation yet.

#![doc(html_no_source)]
