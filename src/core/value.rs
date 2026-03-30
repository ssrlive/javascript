use crate::core::{Collect, Gc, GcCell, GcContext, GcPtr, GcTrace, GcWeak, new_gc_cell_ptr};
use crate::unicode::utf16_to_utf8;
use crate::{
    JSError,
    core::{ClassDefinition, DestructuringElement, Expr, PropertyKey, Statement, VarDeclKind, is_error},
    raise_type_error,
};
use indexmap::IndexMap;
use num_bigint::BigInt;
/// VM Map storage (simple Vec of key-value pairs).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmMapData<'gc> {
    pub entries: Vec<(Value<'gc>, Value<'gc>)>,
    pub is_weak: bool,
}
/// VM Set storage (simple Vec of values).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmSetData<'gc> {
    pub values: Vec<Value<'gc>>,
    pub is_weak: bool,
}
/// Array storage with optional named properties (e.g. `arr.foo = "bar"`).
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct VmArrayData<'gc> {
    pub elements: Vec<Value<'gc>>,
    pub props: IndexMap<String, Value<'gc>>,
}
impl<'gc> VmArrayData<'gc> {
    pub fn new(elements: Vec<Value<'gc>>) -> Self {
        Self {
            elements,
            props: IndexMap::new(),
        }
    }
}
impl<'gc> std::ops::Deref for VmArrayData<'gc> {
    type Target = Vec<Value<'gc>>;
    fn deref(&self) -> &Vec<Value<'gc>> {
        &self.elements
    }
}
impl<'gc> std::ops::DerefMut for VmArrayData<'gc> {
    fn deref_mut(&mut self) -> &mut Vec<Value<'gc>> {
        &mut self.elements
    }
}
use std::sync::{Arc, Mutex};
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSMap<'gc> {
    pub entries: Vec<Option<(Value<'gc>, Value<'gc>)>>,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSSet<'gc> {
    pub values: Vec<Option<Value<'gc>>>,
}
/// A key that can be held weakly: either a GC'd object or a non-registered symbol.
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum WeakKey<'gc> {
    Object(GcWeak<'gc, GcCell<JSObjectData<'gc>>>),
    Symbol(GcWeak<'gc, SymbolData>),
}
impl<'gc> WeakKey<'gc> {
    /// Check if this weak key is still alive and matches the given value.
    pub fn matches(&self, ctx: &crate::core::GcContext<'gc>, val: &Value<'gc>) -> bool {
        match (self, val) {
            (WeakKey::Object(weak), Value::Object(obj)) => weak.upgrade(ctx).is_some_and(|p| Gc::ptr_eq(p, *obj)),
            (WeakKey::Symbol(weak), Value::Symbol(sym)) => weak.upgrade(ctx).is_some_and(|p| Gc::ptr_eq(p, *sym)),
            _ => false,
        }
    }
    /// Check if this weak key is still alive.
    pub fn is_alive(&self, ctx: &crate::core::GcContext<'gc>) -> bool {
        match self {
            WeakKey::Object(weak) => weak.upgrade(ctx).is_some(),
            WeakKey::Symbol(weak) => weak.upgrade(ctx).is_some(),
        }
    }
    /// Create a WeakKey from a Value, returning Err if the value cannot be held weakly.
    pub fn from_value(val: &Value<'gc>) -> Result<WeakKey<'gc>, ()> {
        match val {
            Value::Object(obj) => Ok(WeakKey::Object(Gc::downgrade(*obj))),
            Value::Symbol(sym) => {
                if sym.registered {
                    Err(())
                } else {
                    Ok(WeakKey::Symbol(Gc::downgrade(*sym)))
                }
            }
            _ => Err(()),
        }
    }
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSWeakMap<'gc> {
    pub entries: Vec<(WeakKey<'gc>, Value<'gc>)>,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSWeakSet<'gc> {
    pub values: Vec<WeakKey<'gc>>,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSGenerator<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr<'gc>,
    pub this_val: Option<Value<'gc>>,
    pub args: Vec<Value<'gc>>,
    pub state: GeneratorState<'gc>,
    pub cached_initial_yield: Option<Value<'gc>>,
    pub pending_iterator: Option<JSObjectDataPtr<'gc>>,
    pub pending_iterator_done: bool,
    pub yield_star_iterator: Option<JSObjectDataPtr<'gc>>,
    pub pending_for_await: Option<GeneratorForAwaitState<'gc>>,
    pub pending_for_of: Option<GeneratorForOfState<'gc>>,
    /// When a generator is suspended inside a `finally` block that was entered
    /// due to a throw or return, this holds the pending completion that should
    /// be executed after the finally block finishes.
    pub pending_completion: Option<GeneratorPendingCompletion<'gc>>,
}
/// Represents a deferred completion (throw or return) that is waiting for a
/// `finally` block to finish before being applied.
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum GeneratorPendingCompletion<'gc> {
    Throw(Value<'gc>),
    Return(Value<'gc>),
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct GeneratorForAwaitState<'gc> {
    pub iterator: JSObjectDataPtr<'gc>,
    pub is_async: bool,
    pub decl_kind: Option<VarDeclKind>,
    pub var_name: String,
    pub body: Vec<Statement>,
    pub resume_pc: usize,
    pub awaiting_value: bool,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct GeneratorForOfState<'gc> {
    pub iterator: JSObjectDataPtr<'gc>,
    pub decl_kind: Option<VarDeclKind>,
    pub var_name: String,
    pub body: Vec<Statement>,
    pub resume_pc: usize,
    pub iter_env: JSObjectDataPtr<'gc>,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSAsyncGenerator<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: JSObjectDataPtr<'gc>,
    pub call_env: Option<JSObjectDataPtr<'gc>>,
    pub args: Vec<Value<'gc>>,
    pub state: GeneratorState<'gc>,
    pub cached_initial_yield: Option<Value<'gc>>,
    pub pending: Vec<(GcPtr<'gc, JSPromise<'gc>>, AsyncGeneratorRequest<'gc>)>,
    pub pending_for_await: Option<AsyncForAwaitState<'gc>>,
    pub yield_star_iterator: Option<JSObjectDataPtr<'gc>>,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct AsyncForAwaitState<'gc> {
    pub iterator: JSObjectDataPtr<'gc>,
    pub is_async: bool,
    pub decl_kind: Option<VarDeclKind>,
    pub var_name: String,
    pub yield_expr: Expr,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSProxy<'gc> {
    pub target: Box<Value<'gc>>,
    pub handler: Box<Value<'gc>>,
    pub revoked: bool,
}
#[derive(Clone, Debug, Collect, Default)]
#[collect(require_static)]
pub struct JSArrayBuffer {
    pub data: Arc<Mutex<Vec<u8>>>,
    pub detached: bool,
    pub shared: bool,
    pub max_byte_length: Option<usize>,
    /// Whether this ArrayBuffer is immutable (frozen data, cannot be written to or transferred)
    pub immutable: bool,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSDataView<'gc> {
    pub buffer: GcPtr<'gc, JSArrayBuffer>,
    pub byte_offset: usize,
    pub byte_length: usize,
    /// Whether this DataView was constructed without an explicit byteLength
    /// (i.e. it tracks the buffer's current size minus offset).
    pub length_tracking: bool,
}
#[derive(Clone, Debug, PartialEq, Collect)]
#[collect(require_static)]
pub enum TypedArrayKind {
    Int8,
    Uint8,
    Uint8Clamped,
    Int16,
    Uint16,
    Int32,
    Uint32,
    Float16,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSTypedArray<'gc> {
    pub kind: TypedArrayKind,
    pub buffer: GcPtr<'gc, JSArrayBuffer>,
    pub byte_offset: usize,
    pub length: usize,
    pub length_tracking: bool,
}
#[derive(Clone, Collect, Default)]
#[collect(no_drop)]
pub enum GeneratorState<'gc> {
    #[default]
    NotStarted,
    Running {
        pc: usize,
        stack: Vec<Value<'gc>>,
    },
    Suspended {
        pc: usize,
        stack: Vec<Value<'gc>>,
        pre_env: Option<JSObjectDataPtr<'gc>>,
    },
    Completed,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum AsyncGeneratorRequest<'gc> {
    Next(Value<'gc>),
    Throw(Value<'gc>),
    Return(Value<'gc>),
}
pub type JSObjectDataPtr<'gc> = GcPtr<'gc, JSObjectData<'gc>>;
#[inline]
pub fn new_js_object_data<'gc>(ctx: &GcContext<'gc>) -> JSObjectDataPtr<'gc> {
    new_gc_cell_ptr(ctx, JSObjectData::new())
}
#[derive(Clone, Default)]
pub struct JSObjectData<'gc> {
    pub properties: indexmap::IndexMap<PropertyKey<'gc>, GcPtr<'gc, Value<'gc>>>,
    pub constants: std::collections::HashSet<String>,
    /// Internal attribute sets used by the engine to represent property attributes.
    /// These are implementation-level fast-paths for `enumerable`, `writable` and `configurable`.
    /// They are not directly accessible from JS; use descriptor objects for JS-facing APIs.
    pub non_enumerable: std::collections::HashSet<PropertyKey<'gc>>,
    pub non_writable: std::collections::HashSet<PropertyKey<'gc>>,
    pub non_configurable: std::collections::HashSet<PropertyKey<'gc>>,
    pub prototype: Option<JSObjectDataPtr<'gc>>,
    pub is_function_scope: bool,
    /// Track names that were declared as lexical bindings (let/const/class) on this environment
    pub lexical_declarations: std::collections::HashSet<String>,
    pub extensible: bool,
    pub class_def: Option<GcPtr<'gc, ClassDefinition>>,
    /// Internal slot holding the environment where the class was defined. This SHOULD NOT be
    /// exposed as an own property (avoid inserting a visible "__definition_env" property).
    pub definition_env: Option<JSObjectDataPtr<'gc>>,
    pub home_object: Option<GcCell<JSObjectDataPtr<'gc>>>,
    /// Internal executable closure for function objects (previously stored as an internal property)
    closure: Option<GcPtr<'gc, Value<'gc>>>,
    /// Internal slot: absolute module path for deferred namespace objects.
    pub deferred_module_path: Option<String>,
    /// Internal slot: cache/global environment associated with deferred namespace objects.
    pub deferred_cache_env: Option<JSObjectDataPtr<'gc>>,
    /// Map from ClassMember index to evaluated PropertyKey for computed fields.
    pub comp_field_keys: std::collections::HashMap<usize, PropertyKey<'gc>>,
    /// Cache of per-class private method functions so instances share the same object.
    pub private_methods: std::collections::HashMap<PropertyKey<'gc>, Value<'gc>>,
}
unsafe impl<'gc> Collect<'gc> for JSObjectData<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        for (k, v) in &self.properties {
            k.trace(cc);
            v.trace(cc);
        }
        for (k, v) in &self.comp_field_keys {
            k.trace(cc);
            v.trace(cc);
        }
        for (k, v) in &self.private_methods {
            k.trace(cc);
            v.trace(cc);
        }
        for k in &self.non_enumerable {
            k.trace(cc);
        }
        for k in &self.non_writable {
            k.trace(cc);
        }
        for k in &self.non_configurable {
            k.trace(cc);
        }
        if let Some(p) = &self.prototype {
            p.trace(cc);
        }
        if let Some(cd) = &self.class_def {
            cd.trace(cc);
        }
        if let Some(def_env) = &self.definition_env {
            def_env.trace(cc);
        }
        if let Some(cl) = &self.closure {
            cl.trace(cc);
        }
        if let Some(cache_env) = &self.deferred_cache_env {
            cache_env.trace(cc);
        }
    }
}
/// Type-safe key for engine-internal slots stored on `JSObjectData`.
///
/// Using an enum guarantees zero collision with user-defined JS properties:
/// only Rust engine code can construct `InternalSlot` variants; user JS code
/// always goes through string-based property APIs that write to `properties`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Collect)]
#[collect(require_static)]
pub enum InternalSlot {
    Proto,
    PrimitiveValue,
    NativeCtor,
    IsConstructor,
    Callable,
    ComputedProto,
    ExtendsNull,
    Kind,
    ObjParamPlaceholder,
    Function,
    Instance,
    Caller,
    Super,
    Frame,
    DefinitionEnv,
    NewTarget,
    ThisInitialized,
    OriginGlobal,
    IsArrowFunction,
    IsStrict,
    IsIndirectEval,
    IsDirectEval,
    FnNamePrefix,
    Filepath,
    FileId,
    Line,
    Column,
    ClassDef,
    IsParameterEnv,
    Test262GlobalCodeMode,
    BoundTarget,
    BoundThis,
    BoundArgLen,
    BoundArg(usize),
    Proxy,
    ProxyWrapper,
    RevokeProxy,
    Promise,
    PromiseRuntime,
    PromiseObjId,
    PromiseInternalId,
    ResultPromise,
    State,
    StateEnv,
    Completed,
    Total,
    Results,
    Reason,
    OrigValue,
    OrigReason,
    OnFinally,
    CurrentPromise,
    Index,
    UnhandledRejection,
    PendingUnhandled,
    IntrinsicPromiseProto,
    IntrinsicPromiseCtor,
    AsyncResolve,
    AsyncReject,
    Generator,
    InGenerator,
    Gen,
    P,
    GenThrowVal,
    GenForofSend,
    AsyncGenerator,
    AsyncGeneratorState,
    AsyncGeneratorProto,
    IteratorIndex,
    IteratorKind,
    IteratorArray,
    IteratorMap,
    IteratorSet,
    IteratorString,
    PendingIterator,
    PendingIteratorDone,
    ArrayIteratorPrototype,
    MapIteratorPrototype,
    SetIteratorPrototype,
    StringIteratorPrototype,
    RegExpStringIteratorPrototype,
    IteratorPrototype,
    IteratorHelperPrototype,
    WrapForValidIteratorProto,
    IteratorHelperKind,
    IteratorHelperUnderlying,
    IteratorHelperNextMethod,
    IteratorHelperCallback,
    IteratorHelperCounter,
    IteratorHelperRemaining,
    IteratorHelperDone,
    IteratorHelperExecuting,
    IteratorHelperStarted,
    IteratorHelperInnerIter,
    IteratorHelperInnerNext,
    WrapForValidUnderlying,
    WrapForValidNextMethod,
    ZipIterators,
    ZipNextMethods,
    ZipOpenFlags,
    ZipMode,
    ZipPadding,
    ZipKeys,
    AsyncFunctionCtor,
    AsyncGeneratorFunctionCtor,
    Map,
    Set,
    WeakMap,
    WeakSet,
    Regex,
    Flags,
    SwapGreed,
    Crlf,
    Locale,
    RegexGlobal,
    RegexIgnoreCase,
    RegexMultiline,
    RegexDotAll,
    RegexUnicode,
    RegexSticky,
    RegexHasIndices,
    RegexUnicodeSets,
    IsRegExpPrototype,
    RegExpIteratorMatcher,
    RegExpIteratorString,
    RegExpIteratorGlobal,
    RegExpIteratorUnicode,
    RegExpIteratorDone,
    Timestamp,
    YieldStarNextMethod,
    UnhandledRejectionPromisePtr,
    FnDeleted(String),
    TypedArray,
    ArrayBuffer,
    SharedArrayBuffer,
    DataView,
    BufferObject,
    TypedArrayIterator,
    DetachArrayBuffer,
    ImportMeta,
    ModuleCache,
    ModuleLoading,
    ModuleEvalErrors,
    ModuleAsyncPending,
    ModuleDeferredNsCache,
    ModuleNamespaceCache,
    ModuleDeferPendingPreloads,
    ModuleSourceClassName,
    AbstractModuleSourceCtor,
    DefaultExport,
    IsError,
    IsErrorConstructor,
    IsArray,
    IsArrayConstructor,
    IsBooleanConstructor,
    IsDateConstructor,
    IsStringConstructor,
    ImmutablePrototype,
    GlobalLexEnv,
    AllowDynamicImportResult,
    SuppressDynamicImportResult,
    SymbolRegistry,
    TemplateRegistry,
    ThrowTypeError,
    Eof,
    LookupGetter,
    LookupSetter,
    DisposableResources,
    DisposableType,
    IsRawJSON,
    IsHTMLDDA,
    WeakRefTarget,
    WeakRefMarker,
    FRCleanup,
    FRMarker,
    FRCells,
    ShadowRealm,
    WrappedTarget,
    WrappedCallerRealm,
    WrappedTargetRealm,
    ClassField(String),
    ParamBinding(String),
    ImportSrc(String),
    ReexportSrc(String),
    NsSrc(String),
    GlobalLex(String),
    GenPreExec(String),
    GenYieldVal(String),
    InternalFn(String),
}
/// Convert a `__`-prefixed string key to its typed `InternalSlot` variant.
///
/// This is a **temporary bridge** used during migration.  Once all engine call
/// sites are converted to use `InternalSlot` directly, this function (and the
/// transparent routing that calls it) will be removed.
pub fn str_to_internal_slot(s: &str) -> Option<InternalSlot> {
    if !s.starts_with("__") {
        return None;
    }
    match s {
        "__value__" => return Some(InternalSlot::PrimitiveValue),
        "__native_ctor" => return Some(InternalSlot::NativeCtor),
        "__is_constructor" => return Some(InternalSlot::IsConstructor),
        "__callable__" => return Some(InternalSlot::Callable),
        "__computed_proto" => return Some(InternalSlot::ComputedProto),
        "__extends_null" => return Some(InternalSlot::ExtendsNull),
        "__kind" => return Some(InternalSlot::Kind),
        "__obj_param_placeholder" => return Some(InternalSlot::ObjParamPlaceholder),
        "__function" => return Some(InternalSlot::Function),
        "__instance" => return Some(InternalSlot::Instance),
        "__caller" => return Some(InternalSlot::Caller),
        "__super" => return Some(InternalSlot::Super),
        "__frame" => return Some(InternalSlot::Frame),
        "__definition_env" => return Some(InternalSlot::DefinitionEnv),
        "__new_target" => return Some(InternalSlot::NewTarget),
        "__this_initialized" => return Some(InternalSlot::ThisInitialized),
        "__origin_global" => return Some(InternalSlot::OriginGlobal),
        "__is_arrow_function" => return Some(InternalSlot::IsArrowFunction),
        "__is_strict" => return Some(InternalSlot::IsStrict),
        "__is_indirect_eval" => return Some(InternalSlot::IsIndirectEval),
        "__is_direct_eval" => return Some(InternalSlot::IsDirectEval),
        "__fn_name_prefix" => return Some(InternalSlot::FnNamePrefix),
        "__filepath" => return Some(InternalSlot::Filepath),
        "__file_id" => return Some(InternalSlot::FileId),
        "__line__" => return Some(InternalSlot::Line),
        "__column__" => return Some(InternalSlot::Column),
        "__class_def__" => return Some(InternalSlot::ClassDef),
        "__is_parameter_env" => return Some(InternalSlot::IsParameterEnv),
        "__test262_global_code_mode" => return Some(InternalSlot::Test262GlobalCodeMode),
        "__bound_target" => return Some(InternalSlot::BoundTarget),
        "__bound_this" => return Some(InternalSlot::BoundThis),
        "__bound_arg_len" => return Some(InternalSlot::BoundArgLen),
        "__proxy__" => return Some(InternalSlot::Proxy),
        "__proxy_wrapper" => return Some(InternalSlot::ProxyWrapper),
        "__revoke_proxy" => return Some(InternalSlot::RevokeProxy),
        "__promise" => return Some(InternalSlot::Promise),
        "__promise_runtime" => return Some(InternalSlot::PromiseRuntime),
        "__promise_obj_id" => return Some(InternalSlot::PromiseObjId),
        "__promise_internal_id" => return Some(InternalSlot::PromiseInternalId),
        "__result_promise" => return Some(InternalSlot::ResultPromise),
        "__state" => return Some(InternalSlot::State),
        "__state_env" => return Some(InternalSlot::StateEnv),
        "__completed" => return Some(InternalSlot::Completed),
        "__total" => return Some(InternalSlot::Total),
        "__results" => return Some(InternalSlot::Results),
        "__reason" => return Some(InternalSlot::Reason),
        "__orig_value" => return Some(InternalSlot::OrigValue),
        "__orig_reason" => return Some(InternalSlot::OrigReason),
        "__on_finally" => return Some(InternalSlot::OnFinally),
        "__current_promise" => return Some(InternalSlot::CurrentPromise),
        "__index" => return Some(InternalSlot::Index),
        "__unhandled_rejection" => return Some(InternalSlot::UnhandledRejection),
        "__pending_unhandled" => return Some(InternalSlot::PendingUnhandled),
        "__intrinsic_promise_proto" => return Some(InternalSlot::IntrinsicPromiseProto),
        "__intrinsic_promise_ctor" => return Some(InternalSlot::IntrinsicPromiseCtor),
        "__async_resolve" => return Some(InternalSlot::AsyncResolve),
        "__async_reject" => return Some(InternalSlot::AsyncReject),
        "__generator__" => return Some(InternalSlot::Generator),
        "__in_generator" => return Some(InternalSlot::InGenerator),
        "__gen" => return Some(InternalSlot::Gen),
        "__p" => return Some(InternalSlot::P),
        "__gen_throw_val" => return Some(InternalSlot::GenThrowVal),
        "__gen_forof_send" => return Some(InternalSlot::GenForofSend),
        "__async_generator" => return Some(InternalSlot::AsyncGenerator),
        "__async_generator__" => return Some(InternalSlot::AsyncGeneratorState),
        "__async_generator_proto" => return Some(InternalSlot::AsyncGeneratorProto),
        "__iterator_index__" => return Some(InternalSlot::IteratorIndex),
        "__iterator_kind__" => return Some(InternalSlot::IteratorKind),
        "__iterator_array__" => return Some(InternalSlot::IteratorArray),
        "__iterator_map__" => return Some(InternalSlot::IteratorMap),
        "__iterator_set__" => return Some(InternalSlot::IteratorSet),
        "__iterator_string__" => return Some(InternalSlot::IteratorString),
        "__pending_iterator" => return Some(InternalSlot::PendingIterator),
        "__pending_iterator_done" => return Some(InternalSlot::PendingIteratorDone),
        "__map__" => return Some(InternalSlot::Map),
        "__set__" => return Some(InternalSlot::Set),
        "__weakmap__" => return Some(InternalSlot::WeakMap),
        "__weakset__" => return Some(InternalSlot::WeakSet),
        "__regex" => return Some(InternalSlot::Regex),
        "__flags" => return Some(InternalSlot::Flags),
        "__swapGreed" => return Some(InternalSlot::SwapGreed),
        "__crlf" => return Some(InternalSlot::Crlf),
        "__locale" => return Some(InternalSlot::Locale),
        "__global" => return Some(InternalSlot::RegexGlobal),
        "__ignoreCase" => return Some(InternalSlot::RegexIgnoreCase),
        "__multiline" => return Some(InternalSlot::RegexMultiline),
        "__dotAll" => return Some(InternalSlot::RegexDotAll),
        "__unicode" => return Some(InternalSlot::RegexUnicode),
        "__sticky" => return Some(InternalSlot::RegexSticky),
        "__hasIndices" => return Some(InternalSlot::RegexHasIndices),
        "__unicodeSets" => return Some(InternalSlot::RegexUnicodeSets),
        "__timestamp" => return Some(InternalSlot::Timestamp),
        "__yield_star_next_method" => return Some(InternalSlot::YieldStarNextMethod),
        "__unhandled_rejection_promise_ptr" => {
            return Some(InternalSlot::UnhandledRejectionPromisePtr);
        }
        "__typedarray" => return Some(InternalSlot::TypedArray),
        "__arraybuffer" => return Some(InternalSlot::ArrayBuffer),
        "__sharedarraybuffer" => return Some(InternalSlot::SharedArrayBuffer),
        "__dataview" => return Some(InternalSlot::DataView),
        "__buffer_object" => return Some(InternalSlot::BufferObject),
        "__typedarray_iterator" => return Some(InternalSlot::TypedArrayIterator),
        "__detachArrayBuffer__" => return Some(InternalSlot::DetachArrayBuffer),
        "__import_meta" => return Some(InternalSlot::ImportMeta),
        "__module_cache" => return Some(InternalSlot::ModuleCache),
        "__module_loading" => return Some(InternalSlot::ModuleLoading),
        "__module_eval_errors" => return Some(InternalSlot::ModuleEvalErrors),
        "__module_async_pending" => return Some(InternalSlot::ModuleAsyncPending),
        "__module_deferred_namespace_cache" => {
            return Some(InternalSlot::ModuleDeferredNsCache);
        }
        "__module_namespace_cache" => return Some(InternalSlot::ModuleNamespaceCache),
        "__module_defer_pending_preloads" => {
            return Some(InternalSlot::ModuleDeferPendingPreloads);
        }
        "__module_source_class_name" => return Some(InternalSlot::ModuleSourceClassName),
        "__abstract_module_source_ctor" => {
            return Some(InternalSlot::AbstractModuleSourceCtor);
        }
        "__default_export" => return Some(InternalSlot::DefaultExport),
        "__is_error" => return Some(InternalSlot::IsError),
        "__is_error_constructor" => return Some(InternalSlot::IsErrorConstructor),
        "__is_array" => return Some(InternalSlot::IsArray),
        "__is_array_constructor" => return Some(InternalSlot::IsArrayConstructor),
        "__is_boolean_constructor" => return Some(InternalSlot::IsBooleanConstructor),
        "__is_date_constructor" => return Some(InternalSlot::IsDateConstructor),
        "__is_string_constructor" => return Some(InternalSlot::IsStringConstructor),
        "__global_lex_env" => return Some(InternalSlot::GlobalLexEnv),
        "__allow_dynamic_import_result" => {
            return Some(InternalSlot::AllowDynamicImportResult);
        }
        "__suppress_dynamic_import_result" => {
            return Some(InternalSlot::SuppressDynamicImportResult);
        }
        "__symbol_registry" => return Some(InternalSlot::SymbolRegistry),
        "__template_registry" => return Some(InternalSlot::TemplateRegistry),
        "__throw_type_error" => return Some(InternalSlot::ThrowTypeError),
        "__eof" => return Some(InternalSlot::Eof),
        "__lookupGetter__" => return Some(InternalSlot::LookupGetter),
        "__lookupSetter__" => return Some(InternalSlot::LookupSetter),
        "__weakref_target" => return Some(InternalSlot::WeakRefTarget),
        "__weakref_marker" => return Some(InternalSlot::WeakRefMarker),
        "__fr_cleanup" => return Some(InternalSlot::FRCleanup),
        "__fr_marker" => return Some(InternalSlot::FRMarker),
        "__fr_cells" => return Some(InternalSlot::FRCells),
        "__shadow_realm" => return Some(InternalSlot::ShadowRealm),
        "__wrapped_target" => return Some(InternalSlot::WrappedTarget),
        "__wrapped_caller_realm" => return Some(InternalSlot::WrappedCallerRealm),
        "__wrapped_target_realm" => return Some(InternalSlot::WrappedTargetRealm),
        _ => {}
    }
    if let Some(rest) = s.strip_prefix("__bound_arg_") {
        return rest.parse::<usize>().ok().map(InternalSlot::BoundArg);
    }
    if let Some(rest) = s.strip_prefix("__class_field_") {
        return Some(InternalSlot::ClassField(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__param_binding__") {
        return Some(InternalSlot::ParamBinding(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__import_src_") {
        return Some(InternalSlot::ImportSrc(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__reexport_src_") {
        return Some(InternalSlot::ReexportSrc(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__ns_src_") {
        return Some(InternalSlot::NsSrc(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__global_lex_") {
        return Some(InternalSlot::GlobalLex(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__gen_pre_exec_") {
        return Some(InternalSlot::GenPreExec(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__gen_yield_val_") {
        return Some(InternalSlot::GenYieldVal(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__internal_") {
        return Some(InternalSlot::InternalFn(rest.to_string()));
    }
    if let Some(rest) = s.strip_prefix("__fn_deleted::") {
        return Some(InternalSlot::FnDeleted(rest.to_string()));
    }
    None
}
impl<'gc> JSObjectData<'gc> {
    pub fn new() -> Self {
        JSObjectData::<'_> {
            extensible: true,
            ..JSObjectData::default()
        }
    }
    pub fn insert(&mut self, key: impl Into<PropertyKey<'gc>>, val: GcPtr<'gc, Value<'gc>>) {
        let key = key.into();
        self.properties.insert(key, val);
    }
    pub fn set_const(&mut self, key: String) {
        log::debug!("set_const: obj_ptr={:p} key={}", self as *const _, key);
        self.constants.insert(key);
    }
    pub fn set_lexical(&mut self, key: String) {
        self.lexical_declarations.insert(key);
    }
    pub fn has_lexical(&self, key: &str) -> bool {
        self.lexical_declarations.contains(key)
    }
    pub fn set_non_configurable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        self.non_configurable.insert(key);
    }
    pub fn set_configurable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        self.non_configurable.remove(&key);
    }
    pub fn set_non_writable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        log::debug!("set_non_writable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_writable.insert(key);
    }
    pub fn set_writable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        log::debug!("set_writable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_writable.remove(&key);
    }
    pub fn is_const(&self, key: &str) -> bool {
        self.constants.contains(key)
    }
    pub fn set_property(&mut self, ctx: &GcContext<'gc>, key: impl Into<PropertyKey<'gc>>, val: Value<'gc>) {
        let pk = key.into();
        if let PropertyKey::String(s) = &pk
            && s == "__definition_env"
        {
            if let Value::Object(env_obj) = val {
                self.definition_env = Some(env_obj);
                log::debug!("set_property: stored internal definition_env on obj={:p}", self as *const _);
                return;
            } else {
                log::warn!(
                    "set_property: attempted to set '__definition_env' with non-object value on obj={:p}",
                    self as *const _
                );
            }
        }
        let val_ptr = new_gc_cell_ptr(ctx, val);
        self.insert(pk, val_ptr);
    }
    pub fn get_property(&self, key: impl Into<PropertyKey<'gc>>) -> Option<String> {
        let key = key.into();
        if let Some(val_ptr) = self.properties.get(&key) {
            match &*val_ptr.borrow() {
                Value::String(s) => return Some(utf16_to_utf8(s)),
                Value::Property { value: Some(v), .. } => {
                    if let Value::String(s2) = &*v.borrow() {
                        return Some(utf16_to_utf8(s2));
                    }
                    return None;
                }
                _ => return None,
            }
        }
        if let Some(proto) = &self.prototype
            && let Some(val_ptr) = object_get_key_value(proto, key)
        {
            match &*val_ptr.borrow() {
                Value::String(s) => return Some(utf16_to_utf8(s)),
                Value::Property { value: Some(v), .. } => {
                    if let Value::String(s2) = &*v.borrow() {
                        return Some(utf16_to_utf8(s2));
                    }
                    return None;
                }
                _ => return None,
            }
        }
        None
    }
    pub fn get_message(&self) -> Option<String> {
        if let Some(msg_ptr) = self.properties.get(&PropertyKey::String("message".to_string()))
            && let Value::String(s) = &*msg_ptr.borrow()
        {
            return Some(utf16_to_utf8(s));
        }
        None
    }
    pub fn set_line(&mut self, line: usize, ctx: &GcContext<'gc>) -> Result<(), JSError> {
        let key = PropertyKey::Internal(InternalSlot::Line);
        self.properties
            .entry(key)
            .or_insert_with(|| new_gc_cell_ptr(ctx, Value::Number(line as f64)));
        Ok(())
    }
    pub fn get_line(&self) -> Option<usize> {
        let key = PropertyKey::Internal(InternalSlot::Line);
        if let Some(line_ptr) = self.properties.get(&key)
            && let Value::Number(n) = &*line_ptr.borrow()
        {
            return Some(*n as usize);
        }
        None
    }
    pub fn set_column(&mut self, column: usize, ctx: &GcContext<'gc>) -> Result<(), JSError> {
        let key = PropertyKey::Internal(InternalSlot::Column);
        self.properties
            .entry(key)
            .or_insert_with(|| new_gc_cell_ptr(ctx, Value::Number(column as f64)));
        Ok(())
    }
    pub fn get_column(&self) -> Option<usize> {
        let key = PropertyKey::Internal(InternalSlot::Column);
        if let Some(col_ptr) = self.properties.get(&key)
            && let Value::Number(n) = &*col_ptr.borrow()
        {
            return Some(*n as usize);
        }
        None
    }
    pub fn set_non_enumerable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        log::debug!("set_non_enumerable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_enumerable.insert(key);
    }
    pub fn set_enumerable(&mut self, key: impl Into<PropertyKey<'gc>>) {
        let key = key.into();
        log::debug!("set_enumerable: obj_ptr={:p} key={:?}", self as *const _, key);
        self.non_enumerable.remove(&key);
    }
    pub fn is_configurable(&self, key: impl Into<PropertyKey<'gc>>) -> bool {
        let key = key.into();
        !self.non_configurable.contains(&key)
    }
    pub fn is_writable(&self, key: impl Into<PropertyKey<'gc>>) -> bool {
        let key = key.into();
        !self.non_writable.contains(&key)
    }
    pub fn is_extensible(&self) -> bool {
        self.extensible
    }
    pub fn prevent_extensions(&mut self) {
        self.extensible = false;
    }
    pub fn is_enumerable(&self, key: impl Into<PropertyKey<'gc>>) -> bool {
        let key = key.into();
        !self.non_enumerable.contains(&key)
    }
    pub fn get_home_object(&self) -> Option<GcCell<JSObjectDataPtr<'gc>>> {
        self.home_object.clone()
    }
    pub fn set_home_object(&mut self, home: Option<GcCell<JSObjectDataPtr<'gc>>>) {
        let had = self.home_object.is_some();
        let is_some = home.is_some();
        log::trace!(
            "set_home_object: self_ptr={:p} had_home={} setting_home={}",
            self as *const _,
            had,
            is_some
        );
        self.home_object = home;
    }
    pub fn get_closure(&self) -> Option<GcPtr<'gc, Value<'gc>>> {
        self.closure
    }
    pub fn set_closure(&mut self, closure: Option<GcPtr<'gc, Value<'gc>>>) {
        self.closure = closure;
    }
}
impl<'gc> ClosureData<'gc> {
    pub fn new(
        params: &[DestructuringElement],
        body: &[Statement],
        env: Option<JSObjectDataPtr<'gc>>,
        home_object: Option<JSObjectDataPtr<'gc>>,
    ) -> Self {
        ClosureData {
            params: params.to_vec(),
            body: body.to_vec(),
            env,
            home_object: home_object.map(GcCell::new),
            enforce_strictness_inheritance: true,
            ..ClosureData::default()
        }
    }
}
#[derive(Clone, Debug, Collect)]
#[collect(require_static)]
pub struct SymbolData {
    description: Option<String>,
    /// True for symbols created via `Symbol.for()` (global symbol registry).
    /// Registered symbols cannot be used as WeakMap/WeakSet/WeakRef keys.
    pub registered: bool,
}
impl SymbolData {
    pub fn new(description: Option<&str>) -> Self {
        SymbolData {
            description: description.map(|s| s.to_string()),
            registered: false,
        }
    }
    pub fn new_registered(description: Option<&str>) -> Self {
        SymbolData {
            description: description.map(|s| s.to_string()),
            registered: true,
        }
    }
    pub fn description(&self) -> Option<&str> {
        self.description.as_deref()
    }
}
#[derive(Clone, Collect, Default)]
#[collect(no_drop)]
pub struct ClosureData<'gc> {
    pub params: Vec<DestructuringElement>,
    pub body: Vec<Statement>,
    pub env: Option<JSObjectDataPtr<'gc>>,
    pub home_object: Option<GcCell<JSObjectDataPtr<'gc>>>,
    pub captured_envs: Vec<JSObjectDataPtr<'gc>>,
    pub bound_this: Option<Value<'gc>>,
    pub is_arrow: bool,
    pub is_strict: bool,
    pub native_target: Option<String>,
    pub enforce_strictness_inheritance: bool,
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub struct JSPromise<'gc> {
    pub id: usize,
    pub state: PromiseState<'gc>,
    pub value: Option<Value<'gc>>,
    pub on_fulfilled: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    pub on_rejected: Vec<(Value<'gc>, GcPtr<'gc, JSPromise<'gc>>, Option<JSObjectDataPtr<'gc>>)>,
    /// Whether a rejection handler has been attached or a rejection handler
    /// has already executed for this promise. Used to avoid reporting
    /// unhandled rejections after the promise has been handled.
    pub handled: bool,
}
static UNIQUE_ID_SEED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
pub fn generate_unique_id() -> usize {
    UNIQUE_ID_SEED.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}
impl<'gc> JSPromise<'gc> {
    pub fn new() -> Self {
        Self {
            id: generate_unique_id(),
            state: PromiseState::Pending,
            value: None,
            on_fulfilled: Vec::new(),
            on_rejected: Vec::new(),
            handled: false,
        }
    }
}
impl std::fmt::Debug for JSPromise<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "JSPromise {{ on_fulfilled: {}, on_rejected: {} }}",
            self.on_fulfilled.len(),
            self.on_rejected.len()
        )
    }
}
#[derive(Clone, Collect)]
#[collect(no_drop)]
pub enum PromiseState<'gc> {
    Pending,
    Fulfilled(Value<'gc>),
    Rejected(Value<'gc>),
}
#[derive(Clone)]
pub enum Value<'gc> {
    Number(f64),
    BigInt(Box<BigInt>),
    String(Vec<u16>),
    Boolean(bool),
    Undefined,
    Null,
    Object(JSObjectDataPtr<'gc>),
    Function(String),
    VmFunction(usize, u8),
    VmClosure(usize, u8, crate::core::VmUpvalueCells<'gc>),
    VmArray(crate::core::VmArrayHandle<'gc>),
    VmObject(crate::core::VmObjectHandle<'gc>),
    VmNativeFunction(u8),
    VmMap(crate::core::VmMapHandle<'gc>),
    VmSet(crate::core::VmSetHandle<'gc>),
    Closure(Gc<'gc, ClosureData<'gc>>),
    AsyncClosure(Gc<'gc, ClosureData<'gc>>),
    GeneratorFunction(Option<String>, Gc<'gc, ClosureData<'gc>>),
    AsyncGeneratorFunction(Option<String>, Gc<'gc, ClosureData<'gc>>),
    ClassDefinition(Gc<'gc, ClassDefinition>),
    Getter(Vec<Statement>, JSObjectDataPtr<'gc>, Option<GcCell<JSObjectDataPtr<'gc>>>),
    Setter(
        Vec<DestructuringElement>,
        Vec<Statement>,
        JSObjectDataPtr<'gc>,
        Option<GcCell<JSObjectDataPtr<'gc>>>,
    ),
    Promise(GcPtr<'gc, JSPromise<'gc>>),
    Map(GcPtr<'gc, JSMap<'gc>>),
    Set(GcPtr<'gc, JSSet<'gc>>),
    WeakMap(GcPtr<'gc, JSWeakMap<'gc>>),
    WeakSet(GcPtr<'gc, JSWeakSet<'gc>>),
    Generator(GcPtr<'gc, JSGenerator<'gc>>),
    AsyncGenerator(GcPtr<'gc, JSAsyncGenerator<'gc>>),
    Proxy(Gc<'gc, JSProxy<'gc>>),
    ArrayBuffer(GcPtr<'gc, JSArrayBuffer>),
    DataView(Gc<'gc, JSDataView<'gc>>),
    TypedArray(Gc<'gc, JSTypedArray<'gc>>),
    PrivateName(String, u32),
    /// Internal property representation stored in an object's `properties` map.
    /// Contains either a concrete `value` or accessor `getter`/`setter` functions.
    /// Note: a `Value::Property` is not the same as a JS descriptor object
    /// (which is a `JSObjectDataPtr` containing keys like `value`, `writable`, etc.).
    Property {
        value: Option<GcPtr<'gc, Value<'gc>>>,
        getter: Option<Box<Value<'gc>>>,
        setter: Option<Box<Value<'gc>>>,
    },
    Symbol(Gc<'gc, SymbolData>),
    Uninitialized,
}
impl<'gc> Value<'gc> {
    pub fn is_null_or_undefined(&self) -> bool {
        matches!(self, Value::Null | Value::Undefined)
    }
    pub fn to_truthy(&self) -> bool {
        match self {
            Value::Boolean(b) => *b,
            Value::Number(n) => *n != 0.0 && !n.is_nan(),
            Value::String(s) => !s.is_empty(),
            Value::Null | Value::Undefined | Value::Uninitialized => false,
            Value::BigInt(b) => !num_traits::Zero::is_zero(&**b),
            _ => true,
        }
    }
    pub fn normalize_slot(&self) -> Value<'gc> {
        // match self {
        //     Value::Property { value: Some(v), .. } => v.borrow().clone(),
        //     Value::Property { value: None, .. } => Value::Undefined,
        //     other => other.clone(),
        // }
        todo!()
    }
}

impl From<f64> for Value<'_> {
    fn from(n: f64) -> Self {
        Value::Number(n)
    }
}
impl From<bool> for Value<'_> {
    fn from(b: bool) -> Self {
        Value::Boolean(b)
    }
}
impl From<&str> for Value<'_> {
    fn from(s: &str) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(s))
    }
}
impl From<String> for Value<'_> {
    fn from(s: String) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(&s))
    }
}
impl From<&String> for Value<'_> {
    fn from(s: &String) -> Self {
        Value::String(crate::unicode::utf8_to_utf16(s))
    }
}
unsafe impl<'gc> Collect<'gc> for Value<'gc> {
    fn trace<T: GcTrace<'gc>>(&self, cc: &mut T) {
        match self {
            Value::Object(obj) => obj.trace(cc),
            Value::Closure(cl) => cl.trace(cc),
            Value::AsyncClosure(cl) => cl.trace(cc),
            Value::GeneratorFunction(_, cl) => cl.trace(cc),
            Value::AsyncGeneratorFunction(_, cl) => cl.trace(cc),
            Value::ClassDefinition(cl) => cl.trace(cc),
            Value::Getter(body, env, home_object) => {
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(home_object) = home_object {
                    home_object.trace(cc);
                }
            }
            Value::Setter(param, body, env, home_object) => {
                for p in param {
                    p.trace(cc);
                }
                for s in body {
                    s.trace(cc);
                }
                env.trace(cc);
                if let Some(home_obj) = home_object {
                    home_obj.trace(cc);
                }
            }
            Value::Promise(p) => p.trace(cc),
            Value::Map(m) => m.trace(cc),
            Value::Set(s) => s.trace(cc),
            Value::WeakMap(m) => m.trace(cc),
            Value::WeakSet(s) => s.trace(cc),
            Value::Generator(g) => g.trace(cc),
            Value::AsyncGenerator(g) => g.trace(cc),
            Value::Proxy(p) => p.trace(cc),
            Value::ArrayBuffer(b) => b.trace(cc),
            Value::DataView(d) => d.trace(cc),
            Value::TypedArray(t) => t.trace(cc),
            Value::Property { value, getter, setter } => {
                if let Some(v) = value {
                    v.trace(cc);
                }
                if let Some(g) = getter {
                    g.trace(cc);
                }
                if let Some(s) = setter {
                    s.trace(cc);
                }
            }
            Value::Symbol(sym) => sym.trace(cc),
            _ => {}
        }
    }
}
impl<'gc> std::fmt::Debug for Value<'gc> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Value::Number(n) => write!(f, "Number({})", n),
            Value::String(s) => write!(f, "String({:?})", utf16_to_utf8(s)),
            Value::Boolean(b) => write!(f, "Boolean({})", b),
            Value::Null => write!(f, "Null"),
            Value::Undefined => write!(f, "Undefined"),
            Value::Object(_) => write!(f, "Object"),
            Value::Function(s) => write!(f, "Function({})", s),
            _ => write!(f, "[value]"),
        }
    }
}

thread_local! {
    static VTOS_DEPTH : std::cell::Cell < usize > = const { std::cell::Cell::new(0) };
}
pub fn value_to_string<'gc>(val: &Value<'gc>) -> String {
    let depth = VTOS_DEPTH.with(|d| {
        let cur = d.get();
        d.set(cur + 1);
        cur + 1
    });
    if depth > 10 {
        VTOS_DEPTH.with(|d| d.set(d.get() - 1));
        return "[object]".to_string();
    }
    let res = match val {
        Value::Number(n) => {
            if n.is_nan() {
                "NaN".to_string()
            } else if n.is_infinite() {
                if n.is_sign_negative() {
                    "-Infinity".to_string()
                } else {
                    "Infinity".to_string()
                }
            } else {
                format_js_number(*n)
            }
        }
        Value::BigInt(b) => b.to_string(),
        Value::String(s) => utf16_to_utf8(s),
        Value::Boolean(b) => b.to_string(),
        Value::Undefined => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Object(obj) => {
            if is_error(val) {
                let msg = obj.borrow().get_message().unwrap_or("Unknown error".into());
                return format!("Error: {msg}");
            }
            if let Ok(borrowed) = obj.try_borrow()
                && let Some(msg) = borrowed.get_message()
            {
                return msg;
            }
            "[object Object]".to_string()
        }
        Value::Function(name) => {
            let is_identifier_name = |s: &str| {
                let mut chars = s.chars();
                let Some(first) = chars.next() else {
                    return false;
                };
                let first_ok = first == '_' || first == '$' || first.is_ascii_alphabetic();
                if !first_ok {
                    return false;
                }
                chars.all(|c| c == '_' || c == '$' || c.is_ascii_alphanumeric())
            };
            if name.is_empty() {
                "function () { [native code] }".to_string()
            } else if is_identifier_name(name) || name.starts_with('[') {
                format!("function {name}() {{ [native code] }}")
            } else {
                format!("function [{name}]() {{ [native code] }}")
            }
        }
        Value::Closure(..) => "function".to_string(),
        Value::AsyncClosure(..) => "async function".to_string(),
        Value::GeneratorFunction(name, ..) => {
            format!("function* {}", name.as_deref().unwrap_or(""))
        }
        Value::AsyncGeneratorFunction(name, ..) => {
            format!("async function* {}", name.as_deref().unwrap_or(""))
        }
        Value::ClassDefinition(..) => "class".to_string(),
        Value::Getter(..) => "[Getter]".to_string(),
        Value::Setter(..) => "[Setter]".to_string(),
        Value::PrivateName(n, _) => format!("#{n}"),
        Value::Promise(_) => "[object Promise]".to_string(),
        Value::Map(_) => "[object Map]".to_string(),
        Value::Set(_) => "[object Set]".to_string(),
        Value::WeakMap(_) => "[object WeakMap]".to_string(),
        Value::WeakSet(_) => "[object WeakSet]".to_string(),
        Value::Generator(_) => "[object Generator]".to_string(),
        Value::AsyncGenerator(_) => "[object AsyncGenerator]".to_string(),
        Value::Proxy(_) => "[object Proxy]".to_string(),
        Value::ArrayBuffer(_) => "[object ArrayBuffer]".to_string(),
        Value::DataView(_) => "[object DataView]".to_string(),
        Value::TypedArray(_) => "[object TypedArray]".to_string(),
        Value::Property { .. } => "[Property]".to_string(),
        Value::Symbol(sym) => {
            if let Some(desc) = &sym.description {
                format!("Symbol({desc})")
            } else {
                "Symbol()".to_string()
            }
        }
        Value::Uninitialized => "[uninitialized]".to_string(),
        Value::VmFunction(ip, arity) => format!("[VmFunction@{} arity={}]", ip, arity),
        Value::VmClosure(ip, arity, _) => format!("[VmClosure@{} arity={}]", ip, arity),
        Value::VmArray(arr) => {
            let elems: Vec<String> = arr
                .borrow()
                .iter()
                .map(|v| {
                    let s = value_to_string(v);
                    match v {
                        Value::String(_) => format!("'{s}'"),
                        _ => s,
                    }
                })
                .collect();
            format!("[ {} ]", elems.join(", "))
        }
        Value::VmObject(obj) => {
            {
                let Ok(borrowed) = obj.try_borrow() else {
                    return "[object Object]".to_string();
                };
                if let Some(Value::String(tname)) = borrowed.get("__type__") {
                    let tname_str = crate::unicode::utf16_to_utf8(tname);
                    if tname_str == "RegExp" {
                        return "[object RegExp]".to_string();
                    }
                    if tname_str.ends_with("Error") {
                        let msg = borrowed
                            .get("message")
                            .and_then(|v| if let Value::String(s) = v { Some(utf16_to_utf8(s)) } else { None })
                            .unwrap_or_default();
                        return format!("{}: {}", tname_str, msg);
                    }
                }
                if let Some(Value::String(s)) = borrowed.get("message") {
                    return utf16_to_utf8(s);
                }
            }
            let mut parts = Vec::new();
            if let Ok(borrowed) = obj.try_borrow() {
                for (k, v) in borrowed.iter() {
                    if k.starts_with("__") {
                        continue;
                    }
                    let vs = value_to_string(v);
                    parts.push(format!("{k}: {vs}"));
                }
            }
            format!("{{ {} }}", parts.join(", "))
        }
        Value::VmNativeFunction(id) => format!("[NativeFunction#{}]", id),
        Value::VmMap(m) => {
            if m.borrow().is_weak {
                "[object WeakMap]".to_string()
            } else {
                "[object Map]".to_string()
            }
        }
        Value::VmSet(s) => {
            if s.borrow().is_weak {
                "[object WeakSet]".to_string()
            } else {
                "[object Set]".to_string()
            }
        }
    };
    VTOS_DEPTH.with(|d| d.set(d.get() - 1));
    res
}
pub fn value_to_compact_result_string<'gc>(val: &Value<'gc>) -> String {
    match val {
        Value::Number(_) | Value::BigInt(_) | Value::Boolean(_) | Value::VmFunction(..) | Value::VmClosure(..) => value_to_string(val),
        Value::String(s) => {
            let rust_str = utf16_to_utf8(s);
            format!("\"{}\"", rust_str.replace('\\', "\\\\").replace('"', "\\\""))
        }
        Value::Undefined | Value::Null => "null".to_string(),
        Value::VmArray(arr) => {
            let borrow = arr.borrow();
            let parts: Vec<String> = borrow
                .elements
                .iter()
                .enumerate()
                .map(|(i, v)| {
                    if borrow.props.contains_key(&format!("__deleted_{}", i)) {
                        "null".to_string()
                    } else {
                        value_to_compact_result_string(v)
                    }
                })
                .collect();
            format!("[{}]", parts.join(","))
        }
        Value::VmObject(obj) => {
            let borrow = obj.borrow();
            if let Some(Value::String(t)) = borrow.get("__type__")
                && utf16_to_utf8(t) == "RegExp"
            {
                return "[object RegExp]".to_string();
            }
            if let Some(Value::String(t)) = borrow.get("__type__")
                && utf16_to_utf8(t) == "Promise"
            {
                if let Some(v) = borrow.get("__promise_value__") {
                    if matches!(borrow.get("__promise_rejected__"), Some(Value::Boolean(true))) {
                        return format!("Promise {{ <rejected>: {} }}", value_to_compact_result_string(v));
                    }
                    return format!("Promise {{ <fulfilled>: {} }}", value_to_compact_result_string(v));
                }
                return "Promise { <pending> }".to_string();
            }
            let mut parts: Vec<String> = borrow
                .iter()
                .filter(|(k, _)| !k.starts_with("__"))
                .map(|(k, v)| {
                    let escaped_key = k.replace('\\', "\\\\").replace('"', "\\\"");
                    let rendered = match v {
                        Value::VmObject(_)
                        | Value::VmArray(_)
                        | Value::VmMap(_)
                        | Value::VmSet(_)
                        | Value::VmFunction(..)
                        | Value::VmClosure(..)
                        | Value::VmNativeFunction(_) => value_to_string(v),
                        _ => value_to_compact_result_string(v),
                    };
                    format!("\"{}\":{}", escaped_key, rendered)
                })
                .collect();
            if let Some(Value::VmObject(proto)) = borrow.get("__proto__") {
                let own_keys: std::collections::HashSet<String> = borrow.keys().filter(|k| !k.starts_with("__")).cloned().collect();
                let proto_borrow = proto.borrow();
                for (k, v) in proto_borrow.iter() {
                    if k.starts_with("__") || own_keys.contains(k) {
                        continue;
                    }
                    if !matches!(
                        v,
                        Value::Undefined
                            | Value::Null
                            | Value::Boolean(_)
                            | Value::Number(_)
                            | Value::BigInt(_)
                            | Value::String(_)
                            | Value::Symbol(_)
                    ) {
                        continue;
                    }
                    let escaped_key = k.replace('\\', "\\\\").replace('"', "\\\"");
                    parts.push(format!("\"{}\":{}", escaped_key, value_to_compact_result_string(v)));
                }
            }
            format!("{{{}}}", parts.join(","))
        }
        _ => value_to_string(val),
    }
}
pub fn format_js_number(n: f64) -> String {
    log::debug!(
        "DBG format_js_number: n={} is_zero={} sign_neg={}",
        n,
        n == 0.0,
        n.is_sign_negative()
    );
    if n == 0.0 {
        return "0".to_string();
    }
    if n.to_bits() == 1 {
        return "5e-324".to_string();
    }
    if n == f64::MAX {
        return "1.7976931348623157e+308".to_string();
    }
    let abs = n.abs();
    if !(1e-6..1e21).contains(&abs) {
        let precision = if abs >= 1e21 { 16 } else { 15 };
        let s = format!("{:.*e}", precision, n);
        if let Some((mant, exp)) = s.split_once('e') {
            let mant = mant.trim_end_matches('0').trim_end_matches('.');
            if let Ok(exp_int) = exp.parse::<i32>() {
                return format!("{}e{:+}", mant, exp_int);
            }
        }
        return s;
    }
    let mut s = format!("{}", n);
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s
}
pub fn object_get_key_value<'gc>(obj: &JSObjectDataPtr<'gc>, key: impl Into<PropertyKey<'gc>>) -> Option<GcPtr<'gc, Value<'gc>>> {
    let key = key.into();
    let mut current = Some(*obj);
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(&key) {
            return Some(*val);
        }
        current = cur.borrow().prototype;
    }
    if let Some(global_this_cell) = obj.borrow().properties.get(&PropertyKey::String("globalThis".to_string()))
        && let Value::Object(global_this_obj) = &*global_this_cell.borrow()
        && Gc::ptr_eq(*global_this_obj, *obj)
        && let PropertyKey::String(method_name) = &key
        && matches!(
            method_name.as_str(),
            "hasOwnProperty" | "isPrototypeOf" | "propertyIsEnumerable" | "toLocaleString" | "toString" | "valueOf"
        )
        && let Some(obj_ctor_val) = obj.borrow().properties.get(&PropertyKey::String("Object".to_string()))
        && let Value::Object(obj_ctor) = &*obj_ctor_val.borrow()
        && let Some(proto_val) = object_get_key_value(obj_ctor, "prototype")
        && let Value::Object(proto_obj) = &*proto_val.borrow()
    {
        return object_get_key_value(proto_obj, key);
    }
    None
}
pub fn object_set_key_value<'gc>(
    ctx: &GcContext<'gc>,
    obj: &JSObjectDataPtr<'gc>,
    key: impl Into<PropertyKey<'gc>>,
    val: &Value<'gc>,
) -> Result<(), JSError> {
    let _ = (ctx, obj, key, val);
    unimplemented!("object_set_key_value is currently unused and unimplemented")
}
pub fn env_set<'gc>(ctx: &GcContext<'gc>, env: &JSObjectDataPtr<'gc>, key: &str, val: &Value<'gc>) -> Result<(), JSError> {
    if (*env.borrow()).is_const(key) {
        log::trace!(
            "env_set: assignment to const detected: env_ptr={:p} key={} constants={:?} lexical_decls={:?} own_props={:?}",
            &**env as *const _,
            key,
            env.borrow().constants,
            env.borrow().lexical_declarations,
            env.borrow().properties.keys().collect::<Vec<_>>()
        );
        return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
    }
    let val_ptr = new_gc_cell_ptr(ctx, val.clone());
    let pk = if let Some(slot) = str_to_internal_slot(key) {
        PropertyKey::Internal(slot)
    } else {
        PropertyKey::String(key.to_string())
    };
    let str_pk = PropertyKey::String(key.to_string());
    let has_own_str = env.borrow().properties.contains_key(&str_pk);
    if has_own_str {
        env.borrow_mut(ctx).insert(str_pk, val_ptr);
        return Ok(());
    }
    let has_own = env.borrow().properties.contains_key(&pk);
    if has_own {
        env.borrow_mut(ctx).insert(pk, val_ptr);
        return Ok(());
    }
    let mut cur = env.borrow().prototype;
    while let Some(c) = cur {
        if c.borrow().is_const(key) {
            return Err(raise_type_error!(format!("Assignment to constant variable '{key}'")));
        }
        let found_str = c.borrow().properties.contains_key(&str_pk);
        if found_str {
            c.borrow_mut(ctx).insert(str_pk, val_ptr);
            return Ok(());
        }
        let found = c.borrow().properties.contains_key(&pk);
        if found {
            c.borrow_mut(ctx).insert(pk, val_ptr);
            return Ok(());
        }
        cur = c.borrow().prototype;
    }
    env.borrow_mut(ctx).insert(pk, val_ptr);
    Ok(())
}

/// Read an internal slot (own only) using a typed `InternalSlot` key.
#[inline]
pub fn slot_get<'gc>(obj: &JSObjectDataPtr<'gc>, slot: &InternalSlot) -> Option<GcPtr<'gc, Value<'gc>>> {
    let key = PropertyKey::Internal(slot.clone());
    obj.borrow().properties.get(&key).copied()
}
/// Walk prototype chain looking for an internal slot (typed key).
#[inline]
pub fn slot_get_chained<'gc>(obj: &JSObjectDataPtr<'gc>, slot: &InternalSlot) -> Option<GcPtr<'gc, Value<'gc>>> {
    let key = PropertyKey::Internal(slot.clone());
    let mut current = Some(*obj);
    while let Some(cur) = current {
        if let Some(val) = cur.borrow().properties.get(&key) {
            return Some(*val);
        }
        current = cur.borrow().prototype;
    }
    None
}

pub fn object_get_length<'gc>(obj: &JSObjectDataPtr<'gc>) -> Option<usize> {
    if let Some(len_ptr) = object_get_key_value(obj, "length") {
        let len_val = len_ptr.borrow();
        match &*len_val {
            Value::Number(n) => return Some(*n as usize),
            Value::Property { value: Some(inner), .. } => {
                if let Value::Number(n) = &*inner.borrow() {
                    return Some(*n as usize);
                }
            }
            _ => {}
        }
    }
    None
}
