initSidebarItems({"enum":[["Abi",""],["AttrStyle","Distinguishes between Attributes that decorate items and Attributes that are contained as statements within items. These two cases need to be distinguished for pretty-printing."],["BinOp",""],["Body",""],["ConstExpr",""],["FloatTy",""],["FunctionRetTy",""],["IntTy",""],["Lit","Literal kind."],["MetaItem","A compile-time attribute item."],["Mutability",""],["NestedMetaItem","Possible values inside of compile-time attribute lists."],["PathParameters","Parameters of a path segment."],["StrStyle",""],["TraitBoundModifier","A modifier on a bound, currently this is only used for `?Sized`, where the modifier is `Maybe`. Negative bounds should also be handled here."],["Ty","The different kinds of types recognized by the compiler"],["TyParamBound","The AST represents all type param bounds as types. `typeck::collect::compute_bounds` matches these against the \"special\" built-in traits (see `middle::lang_items`) and detects Copy, Send and Sync."],["UnOp",""],["Unsafety",""],["VariantData",""],["Visibility",""],["WherePredicate","A single predicate in a `where` clause"]],"fn":[["parse_ident",""],["parse_macro_input",""],["parse_path",""],["parse_type",""],["parse_where_clause",""]],"struct":[["AngleBracketedParameterData","A path like `Foo<'a, T>`"],["Attribute","Doc-comments are promoted to attributes that have `is_sugared_doc` = true"],["BareFnArg","An argument in a function type."],["BareFnTy",""],["Field",""],["Generics","Represents lifetimes and type parameters attached to a declaration of a function, enum, trait, etc."],["Ident",""],["ImplGenerics","Returned by `Generics::split_for_impl`."],["Lifetime",""],["LifetimeDef","A lifetime definition, e.g. `'a: 'b+'c+'d`"],["MacroInput",""],["MutTy",""],["ParenthesizedParameterData","A path like `Foo(A,B) -> C`"],["Path","A \"Path\" is essentially Rust's notion of a name."],["PathSegment","A segment of a path: an identifier, an optional lifetime, and a set of types."],["PolyTraitRef",""],["QSelf","The explicit Self type in a \"qualified path\". The actual path, including the trait and the associated item, is stored separately. `position` represents the index of the associated item qualified with this Self type."],["TyGenerics","Returned by `Generics::split_for_impl`."],["TyParam",""],["TypeBinding","Bind a type to an associated type: `A=Foo`."],["Variant",""],["WhereBoundPredicate","A type bound."],["WhereClause","A `where` clause in a definition"],["WhereRegionPredicate","A lifetime predicate."]]});