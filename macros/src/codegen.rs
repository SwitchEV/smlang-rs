// Move guards to return a Result

use crate::parser::{lifetimes::Lifetimes, AsyncIdent, ParsedStateMachine};
use proc_macro2::Span;
use quote::{format_ident, quote};
use syn::{punctuated::Punctuated, token::Paren, Type, TypeTuple};

pub fn generate_code(sm: &ParsedStateMachine) -> proc_macro2::TokenStream {
    let (sm_name, sm_name_span) = sm
        .name
        .as_ref()
        .map(|name| (name.to_string(), name.span()))
        .unwrap_or_else(|| (String::new(), Span::call_site()));
    let states_type_name = format_ident!("{sm_name}States", span = sm_name_span);
    let events_type_name = format_ident!("{sm_name}Events", span = sm_name_span);
    let error_type_name = format_ident!("{sm_name}Error", span = sm_name_span);
    let state_machine_type_name = format_ident!("{sm_name}StateMachine", span = sm_name_span);
    let state_machine_context_type_name =
        format_ident!("{sm_name}StateMachineContext", span = sm_name_span);

    // Get only the unique states
    let mut state_list: Vec<_> = sm.states.values().collect();
    state_list.sort_by_key(|state| state.to_string());

    let state_list: Vec<_> = state_list
        .iter()
        .map(
            |value| match sm.state_data.data_types.get(&value.to_string()) {
                None => {
                    quote! {
                        #value
                    }
                }
                Some(t) => {
                    quote! {
                        #value(#t)
                    }
                }
            },
        )
        .collect();

    // Extract events
    let mut event_list: Vec<_> = sm.events.values().collect();
    event_list.sort_by_key(|event| event.to_string());

    // Extract events
    let event_list: Vec<_> = event_list
        .iter()
        .map(
            |value| match sm.event_data.data_types.get(&value.to_string()) {
                None => {
                    quote! {
                        #value
                    }
                }
                Some(t) => {
                    quote! {
                        #value(#t)
                    }
                }
            },
        )
        .collect();

    let transitions = &sm.states_events_mapping;

    let in_states: Vec<_> = transitions
        .iter()
        .map(|(name, _)| {
            let state_name = sm.states.get(name).unwrap();

            match sm.state_data.data_types.get(name) {
                None => {
                    quote! {
                        #state_name
                    }
                }
                Some(_) => {
                    quote! {
                        #state_name(state_data)
                    }
                }
            }
        })
        .collect();

    let events: Vec<Vec<_>> = transitions
        .iter()
        .map(|(_, value)| {
            value
                .iter()
                .map(|(name, value)| {
                    let value = &value.event;

                    match sm.event_data.data_types.get(name) {
                        None => {
                            quote! {
                                #value
                            }
                        }
                        Some(_) => {
                            quote! {
                                #value(event_data)
                            }
                        }
                    }
                })
                .collect()
        })
        .collect();

    // println!("sm: {:#?}", sm);
    // println!("in_states: {:#?}", in_states);
    // println!("events: {:#?}", events);
    // println!("transitions: {:#?}", transitions);

    // Map guards, actions and output states into code blocks
    let guards: Vec<Vec<_>> = transitions
        .iter()
        .map(|(_, value)| value.iter().map(|(_, value)| &value.guard).collect())
        .collect();

    let actions: Vec<Vec<_>> = transitions
        .iter()
        .map(|(_, value)| value.iter().map(|(_, value)| &value.action).collect())
        .collect();

    let guard_action_parameters: Vec<Vec<_>> = transitions
        .iter()
        .map(|(name, value)| {
            let state_name = &sm.states.get(name).unwrap().to_string();

            value
                .iter()
                .map(|(name, _)| {
                    let state_data = match sm.state_data.data_types.get(state_name) {
                        Some(_) => quote! { state_data },
                        None => quote! {},
                    };

                    let event_data = match sm.event_data.data_types.get(name) {
                        Some(_) => quote! { event_data },
                        None => quote! {},
                    };

                    if state_data.is_empty() || event_data.is_empty() {
                        quote! { #state_data #event_data }
                    } else {
                        quote! { #state_data, #event_data }
                    }
                })
                .collect()
        })
        .collect();

    let guard_action_ref_parameters: Vec<Vec<_>> = transitions
        .iter()
        .map(|(name, value)| {
            let state_name = &sm.states.get(name).unwrap().to_string();

            value
                .iter()
                .map(|(name, _)| {
                    let state_data = match sm.state_data.data_types.get(state_name) {
                        Some(Type::Reference(_)) => quote! { state_data },
                        Some(_) => quote! { &state_data },
                        None => quote! {},
                    };

                    let event_data = match sm.event_data.data_types.get(name) {
                        Some(Type::Reference(_)) => quote! { event_data },
                        Some(_) => quote! { &event_data },
                        None => quote! {},
                    };

                    if state_data.is_empty() || event_data.is_empty() {
                        quote! { #state_data #event_data }
                    } else {
                        quote! { #state_data, #event_data }
                    }
                })
                .collect()
        })
        .collect();

    let out_states: Vec<Vec<_>> = transitions
        .iter()
        .map(|(_, value)| {
            value
                .iter()
                .map(|(_, value)| {
                    let out_state = &value.out_state;

                    match sm.state_data.data_types.get(&out_state.to_string()) {
                        None => {
                            quote! {
                                #out_state
                            }
                        }
                        Some(_) => {
                            quote! {
                                #out_state(_data)
                            }
                        }
                    }
                })
                .collect()
        })
        .collect();

    let temporary_context = match &sm.temporary_context_type {
        Some(tct) => {
            quote! { temporary_context: #tct, }
        }
        None => {
            quote! {}
        }
    };

    // Keep track of already added actions not to duplicate definitions
    let mut action_set: Vec<syn::Ident> = Vec::new();
    let mut guard_set: Vec<syn::Ident> = Vec::new();

    let mut guard_list = proc_macro2::TokenStream::new();
    let mut action_list = proc_macro2::TokenStream::new();
    for (state, value) in transitions.iter() {
        // create the state data token stream
        let state_data = match sm.state_data.data_types.get(state) {
            Some(st @ Type::Reference(_)) => quote! { state_data: #st, },
            Some(st) => quote! { state_data: &#st, },
            None => quote! {},
        };

        value.iter().for_each(|(event, value)| {
            // get input state lifetimes
            let in_state_lifetimes = sm.state_data.lifetimes.get(&value.in_state.to_string()).cloned().unwrap_or_default();

            // get output state lifetimes
            let out_state_lifetimes = sm.state_data.lifetimes.get(&value.out_state.to_string()).cloned().unwrap_or_default();

            // get event lifetimes
            let event_lifetimes = sm.event_data.lifetimes.get(event).cloned().unwrap_or_default();

            // combine all lifetimes
            let mut all_lifetimes = Lifetimes::new();
            all_lifetimes.extend(&in_state_lifetimes);
            all_lifetimes.extend(&out_state_lifetimes);
            all_lifetimes.extend(&event_lifetimes);

            // Create the guard traits for user implementation
            if let Some(AsyncIdent {ident: guard, is_async}) = &value.guard {
                let event_data = match sm.event_data.data_types.get(event) {
                    Some(et @ Type::Reference(_)) => quote! { event_data: #et },
                    Some(et) => quote! { event_data: &#et },
                    None => quote! {},
                };

                let guard_error = if sm.custom_guard_error {
                    quote! { Self::GuardError }
                } else {
                    quote! { () }
                };

                // Only add the guard if it hasn't been added before
                if !guard_set.iter().any(|g| g == guard) {
                    guard_set.push(guard.clone());
                    let is_async = match is_async {
                        true => quote!{ async },
                        false => quote!{ },
                    };
                    guard_list.extend(quote! {
                        #[allow(missing_docs)]
                        #is_async fn #guard <#all_lifetimes> (&mut self, #temporary_context #state_data #event_data) -> Result<(), #guard_error>;
                    });
                }
            }

            // Create the action traits for user implementation
            if let Some(AsyncIdent {ident: action, is_async}) = &value.action {
                let is_async = match is_async {
                    true => quote!{ async },
                    false => quote!{ },
                };

                let return_type = if let Some(output_data) =
                    sm.state_data.data_types.get(&value.out_state.to_string())
                {
                    output_data.clone()
                } else {
                    // Empty return type
                    Type::Tuple(TypeTuple {
                        paren_token: Paren {
                            span: Span::call_site(),
                        },
                        elems: Punctuated::new(),
                    })
                };

                let state_data = match sm.state_data.data_types.get(state) {
                    Some(st) => {
                        quote! { state_data: #st, }
                    }
                    None => {
                        quote! {}
                    }
                };
                let event_data = match sm.event_data.data_types.get(event) {
                    Some(et) => {
                        quote! { event_data: #et }
                    }
                    None => {
                        quote! {}
                    }
                };

                // Only add the action if it hasn't been added before
                if !action_set.iter().any(|a| a == action) {
                    action_set.push(action.clone());
                    action_list.extend(quote! {
                        #[allow(missing_docs)]
                        #is_async fn #action <#all_lifetimes> (&mut self, #temporary_context #state_data #event_data) -> #return_type;
                    });
                }
            }
        })
    }

    let temporary_context_call = match &sm.temporary_context_type {
        Some(_) => {
            quote! { temporary_context, }
        }
        None => {
            quote! {}
        }
    };

    let mut sm_is_async = false;

    // Create the code blocks inside the switch cases
    let code_blocks: Vec<Vec<_>> = guards
        .iter()
        .zip(
            actions
                .iter()
                .zip(in_states.iter().zip(out_states.iter().zip(guard_action_parameters.iter().zip(guard_action_ref_parameters.iter())))),
        )
        .map(
            |(guards, (actions, (in_state, (out_states, (guard_action_parameters, guard_action_ref_parameters)))))| {
                guards
                    .iter()
                    .zip(
                        actions
                            .iter()
                            .zip(out_states.iter().zip(guard_action_parameters.iter().zip(guard_action_ref_parameters.iter()))),
                    )
                    .map(|(guard, (action, (out_state, (g_a_param, g_a_ref_param))))| {
                        if let Some(AsyncIdent {ident: g, is_async: is_g_async}) = guard {
                            let guard_await = match is_g_async {
                                true => { sm_is_async = true; quote! { .await } },
                                false => quote! { },
                            };
                            if let Some(AsyncIdent {ident: a, is_async: is_a_async}) = action {
                                let action_await = match is_a_async {
                                    true => { sm_is_async = true; quote! { .await } },
                                    false => quote! { },
                                };
                                quote! {
                                    let guard_result = self.context.#g(#temporary_context_call #g_a_ref_param) #guard_await;
                                    self.context.log_guard(stringify!(#g), &guard_result);
                                    if let Err(e) = guard_result {
                                        self.state = Some(#states_type_name::#in_state);
                                        return Err(#error_type_name::GuardFailed(e));
                                    }
                                    let _data = self.context.#a(#temporary_context_call #g_a_param) #action_await;
                                    self.context.log_action(stringify!(#a));
                                    let out_state = #states_type_name::#out_state;
                                    self.context.log_state_change(&out_state);
                                    self.state = Some(out_state);
                                }
                            } else {
                                quote! {
                                    let guard_result = self.context.#g(#temporary_context_call #g_a_ref_param);
                                    self.context.log_guard(stringify!(#g), &guard_result);
                                    if let Err(e) = guard_result {
                                        self.state = Some(#states_type_name::#in_state);
                                        return Err(#error_type_name::GuardFailed(e));
                                    }
                                    let out_state = #states_type_name::#out_state;
                                    self.context.log_state_change(&out_state);
                                    self.state = Some(out_state);
                                }
                            }
                        } else if let Some(AsyncIdent {ident: a, is_async: is_a_async}) = action {
                            let action_await = match is_a_async {
                                true => { sm_is_async = true; quote! { .await } },
                                false => quote! { },
                            };
                            quote! {
                                let _data = self.context.#a(#temporary_context_call #g_a_param) #action_await ;
                                self.context.log_action(stringify!(#a));
                                let out_state = #states_type_name::#out_state;
                                self.context.log_state_change(&out_state);
                                self.state = Some(out_state);
                            }
                        } else {
                            quote! {
                                let out_state = #states_type_name::#out_state;
                                self.context.log_state_change(&out_state);
                                self.state = Some(out_state);
                            }
                        }
                    })
                    .collect()
            },
        )
        .collect();

    let starting_state = &sm.starting_state;

    // create a token stream for creating a new machine.  If the starting state contains data, then
    // add a second argument to pass this initial data
    let starting_state_name = starting_state.to_string();
    let new_sm_code = match sm.state_data.data_types.get(&starting_state_name) {
        Some(st) => quote! {
            pub const fn new(context: T, state_data: #st ) -> Self {
                #state_machine_type_name {
                    state: Some(#states_type_name::#starting_state (state_data)),
                    context
                }
            }
        },
        None => quote! {
            pub const fn new(context: T ) -> Self {
                #state_machine_type_name {
                    state: Some(#states_type_name::#starting_state),
                    context
                }
            }
        },
    };

    let state_lifetimes = &sm.state_data.all_lifetimes;
    let event_lifetimes = &sm.event_data.all_lifetimes;

    // lifetimes that exists in #events_type_name but not in #states_type_name
    let event_unique_lifetimes = event_lifetimes - state_lifetimes;

    let guard_error = if sm.custom_guard_error {
        quote! {
            /// The error type returned by guard functions.
            type GuardError: core::fmt::Debug;
        }
    } else {
        quote! {}
    };

    let guard_error_type = if sm.custom_guard_error {
        quote! { Self::GuardError }
    } else {
        quote! { () }
    };

    let (is_async, is_async_trait) = if sm_is_async {
        (quote! { async }, quote! { #[smlang::async_trait] })
    } else {
        (quote! {}, quote! {})
    };

    let error_type = if sm.custom_guard_error {
        quote! {
            #error_type_name<<T as #state_machine_context_type_name>::GuardError>
        }
    } else {
        quote! {#error_type_name}
    };

    let derive_states_list = &sm.derive_states;
    let derive_events_list = &sm.derive_events;
    // Build the states and events output
    quote! {
        /// This trait outlines the guards and actions that need to be implemented for the state
        /// machine.
        #is_async_trait
        pub trait #state_machine_context_type_name {
            #guard_error
            #guard_list
            #action_list

            /// Called at the beginning of a state machine's `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_process_event(&self, current_state: & #states_type_name, event: & #events_type_name) {}

            /// Called after executing a guard during `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_guard(&self, guard: &'static str, result: &Result<(), #guard_error_type >) {}

            /// Called after executing an action during `process_event()`. No-op by
            /// default but can be overridden in implementations of a state machine's
            /// `StateMachineContext` trait.
            fn log_action(&self, action: &'static str) {}

            /// Called when transitioning to a new state as a result of an event passed to
            /// `process_event()`. No-op by default but can be overridden in implementations
            /// of a state machine's `StateMachineContext` trait.
            fn log_state_change(&self, new_state: & #states_type_name) {}
        }

        /// List of auto-generated states.
        #[allow(missing_docs)]
        #[derive(#(#derive_states_list),*)]
        pub enum #states_type_name <#state_lifetimes> { #(#state_list),* }

        /// Manually define PartialEq for #states_type_name based on variant only to address issue-#21
        impl<#state_lifetimes> PartialEq for #states_type_name <#state_lifetimes> {
            fn eq(&self, other: &Self) -> bool {
                use core::mem::discriminant;
                discriminant(self) == discriminant(other)
            }
        }

        /// List of auto-generated events.
        #[allow(missing_docs)]
        #[derive(#(#derive_events_list),*)]
        pub enum #events_type_name <#event_lifetimes> { #(#event_list),* }

        /// Manually define PartialEq for #events_type_name based on variant only to address issue-#21
        impl<#event_lifetimes> PartialEq for #events_type_name <#event_lifetimes> {
            fn eq(&self, other: &Self) -> bool {
                use core::mem::discriminant;
                discriminant(self) == discriminant(other)
            }
        }

        /// List of possible errors
        #[derive(Debug)]
        pub enum #error_type_name <T=()> {
            /// When an event is processed which should not come in the current state.
            InvalidEvent,
            /// When an event is processed whose guard did not return `true`.
            GuardFailed(T),
            /// When the state has an unexpected value.
            ///
            /// This can happen if there is a bug in the code generated by smlang,
            /// or if a guard or action gets panicked.
            Poisoned,
        }

        /// State machine structure definition.
        pub struct #state_machine_type_name<#state_lifetimes T: #state_machine_context_type_name> {
            state: Option<#states_type_name <#state_lifetimes>>,
            context: T
        }

        impl<#state_lifetimes T: #state_machine_context_type_name> #state_machine_type_name<#state_lifetimes T> {
            /// Creates a new state machine with the specified starting state.
            #[inline(always)]
            #new_sm_code

            /// Creates a new state machine with an initial state.
            #[inline(always)]
            pub const fn new_with_state(context: T, initial_state: #states_type_name <#state_lifetimes>) -> Self {
                #state_machine_type_name {
                    state: Some(initial_state),
                    context
                }
            }

            /// Returns the current state.
            #[inline(always)]
            pub fn state(&self) -> Result<&#states_type_name <#state_lifetimes>, #error_type> {
                self.state.as_ref().ok_or_else(|| #error_type_name ::Poisoned)
            }

            /// Returns the current context.
            #[inline(always)]
            pub fn context(&self) -> &T {
                &self.context
            }

            /// Returns the current context as a mutable reference.
            #[inline(always)]
            pub fn context_mut(&mut self) -> &mut T {
                &mut self.context
            }

            /// Process an event.
            ///
            /// It will return `Ok(&NextState)` if the transition was successful, or `Err(#error_type_name)`
            /// if there was an error in the transition.
            pub #is_async fn process_event <#event_unique_lifetimes> (
                &mut self,
                #temporary_context
                mut event: #events_type_name <#event_lifetimes>
            ) -> Result<&#states_type_name <#state_lifetimes>, #error_type> {
                self.context.log_process_event(self.state()?, &event);
                match self.state.take().ok_or_else(|| #error_type_name ::Poisoned)? {
                    #(#states_type_name::#in_states => match event {
                        #(#events_type_name::#events => {
                            #code_blocks

                            self.state()
                        }),*
                        _ => {
                            self.state = Some(#states_type_name::#in_states);
                            Err(#error_type_name ::InvalidEvent)
                        }
                    }),*
                    state => {
                        self.state = Some(state);
                        Err(#error_type_name ::InvalidEvent)
                    }
                }
            }
        }
    }
}
