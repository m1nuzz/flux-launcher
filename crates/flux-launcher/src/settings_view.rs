Element::col()
        .fill()
        .padding(24)
        .spacing(14)
        .corner(20.0)
        .bg(Color::rgba(0, 0, 0, 0))
        .border(Color::rgba(0, 0, 0, 0), 0)
        .child(
            Element::row()
                .width_match()
                .child(
                    Element::col()
                        .weight(1.0)
                        .spacing(3)
                        .child(
                            Element::label(i18n_hub.tr(|| t!("settings.title").into_owned()))
                                .font_size(25.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.apply_immediate").into_owned()),
                            )
                            .font_size(12.0)
                            .fg(Color::rgba(235, 241, 255, 180)),
                        ),
                )
                .child(Element::segmented_signal(
                    i18n_hub.tr_vec(|| {
                        vec![
                            t!("settings.tab.general").to_string(),
                            t!("settings.tab.visual").to_string(),
                            t!("settings.tab.priorities").to_string(),
                            t!("settings.tab.plugins").to_string(),
                        ]
                    }),
                    settings_tab,
                ))
                .child(
                    Element::button(i18n_hub.tr(|| t!("settings.back").into_owned()))
                        .neutral()
                        .on_click({
                            let cancel_settings = Rc::clone(&cancel_settings);
                            move |ctx| {
                                cancel_settings();
                                ctx.show_window();
                            }
                        }),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 0)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.activation_key").into_owned()),
                            Element::col()
                                .width_match()
                                .spacing(6)
                                .child(
                                    Element::row()
                                        .width_match()
                                        .spacing(8)
                                        .child(
                                            Element::label_signal(activation_display_for_ui)
                                                .width_match()
                                                .padding_xy(10, 8)
                                                .bg(Color::rgba(255, 255, 255, 24))
                                                .corner(8.0),
                                        )
                                        .child(
                                            Element::button(
                                                i18n_hub
                                                    .tr(|| t!("settings.record_key").into_owned()),
                                            )
                                            .neutral()
                                            .on_click(
                                                move |ctx| {
                                                    activation_recording_for_record_button
                                                        .set(true);
                                                    activation_handle_for_record_button
                                                        .set_enabled(false);
                                                    ctx.toast_ok(t!("settings.press_desired_key"));
                                                },
                                            ),
                                        ),
                                )
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.record_hint").into_owned()),
                                    )
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 170))
                                    .visible_when(move || {
                                        activation_recording_for_record_button.get()
                                    }),
                                ),
                        ))
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.ctrl").into_owned()),
                                    activation_ctrl,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.alt").into_owned()),
                                    activation_alt,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.shift").into_owned()),
                                    activation_shift,
                                ))
                                .child(Element::checkbox(
                                    i18n_hub.tr(|| t!("settings.modifier.windows").into_owned()),
                                    activation_meta,
                                )),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.fullscreen_protection").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.fullscreen_protection_desc").into_owned()),
                                ignore_fullscreen,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.game_mode").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.game_mode_desc").into_owned()),
                                game_mode,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.keyboard_layout").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.keyboard_layout_desc").into_owned()),
                                switch_to_english_layout,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.query_on_activation").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.query_on_activation_desc").into_owned()),
                                clear_query_on_activation,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.windows_startup").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.windows_startup_desc").into_owned()),
                                start_with_windows,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.language").into_owned()),
                            Element::dropdown_signal(
                                i18n_hub.tr_vec(|| {
                                    vec![
                                        t!("settings.language_options.follow_system").into_owned(),
                                        t!("settings.language_options.english").into_owned(),
                                        t!("settings.language_options.chinese").into_owned(),
                                    ]
                                }),
                                language_preference,
                            )
                            .on_dropdown_change({
                                let i18n_hub = i18n_hub.clone();
                                move |ctx, index| {
                                    let target_lang = language_preference_from_index(index);
                                    apply_configured_locale(target_lang);
                                    i18n_hub.refresh();
                                    ctx.mark_dirty_all();
                                }
                            }),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.open_launcher_on").into_owned()),
                            Element::col()
                                .spacing(6)
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.primary").into_owned()),
                                    monitor_preference,
                                    0,
                                ))
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.cursor").into_owned()),
                                    monitor_preference,
                                    1,
                                ))
                                .child(Element::radio(
                                    i18n_hub.tr(|| t!("settings.monitor.foreground").into_owned()),
                                    monitor_preference,
                                    2,
                                )),
                        ))
                        .child(
                            Element::col()
                                .width_match()
                                .spacing(8)
                                .child(Element::field_signal(
                                    i18n_hub.tr(|| t!("settings.updates").into_owned()),
                                    Element::checkbox(
                                        i18n_hub.tr(|| t!("settings.updates_desc").into_owned()),
                                        update_checks_enabled,
                                    ),
                                ))
                                .child(
                                    Element::row()
                                        .width_match()
                                        .spacing(8)
                                        .child(
                                            Element::text_input(update_interval_hours, "24")
                                                .width_match(),
                                        )
                                        .child(
                                            Element::label(i18n_hub.tr(|| {
                                                t!("settings.hours_between_checks").into_owned()
                                            }))
                                            .font_size(11.0),
                                        ),
                                )
                                .child(
                                    Element::row()
                                        .width_match()
                                        .spacing(8)
                                        .child(
                                            Element::label(
                                                i18n_hub.tr(|| {
                                                    t!("settings.update_action").into_owned()
                                                }),
                                            )
                                            .width_match(),
                                        )
                                        .child(
                                            Element::label(t!(
                                                "settings.current_version",
                                                version = CURRENT_VERSION
                                            ))
                                            .font_size(11.0)
                                            .fg(Color::rgba(235, 241, 255, 190)),
                                        ),
                                )
                                .child(Element::checkbox(
                                    i18n_hub
                                        .tr(|| t!("settings.auto_install_updates").into_owned()),
                                    auto_install_updates,
                                ))
                                .child(
                                    Element::row()
                                        .width_match()
                                        .spacing(8)
                                        .child(
                                            Element::label_signal(update_status)
                                                .font_size(11.0)
                                                .fg(Color::rgba(235, 241, 255, 190))
                                                .max_lines(2)
                                                .truncate(Truncate::End)
                                                .width_match(),
                                        )
                                        .child(
                                            Element::button(
                                                i18n_hub.tr(|| {
                                                    t!("settings.check_updates").into_owned()
                                                }),
                                            )
                                            .on_click(
                                                move |ctx| {
                                                    update_status_for_apply.set(
                                                        t!("updater.checking_github").into_owned(),
                                                    );
                                                    request_update_check(
                                                        update_sender_for_check_now.clone(),
                                                        &update_check_in_flight_for_check_now,
                                                    );
                                                    ctx.toast_ok(t!("updater.checking_toast"));
                                                },
                                            ),
                                        )
                                        .child(
                                            Element::button(
                                                i18n_hub
                                                    .tr(|| t!("settings.install_now").into_owned()),
                                            )
                                            .visible_when(move || {
                                                update_available_for_install.get().is_some()
                                                    && !update_installing_for_ui.get()
                                            })
                                            .on_click(
                                                move |ctx| {
                                                    if update_installing_for_ui.get() {
                                                        return;
                                                    }
                                                    if let Some(update) =
                                                        update_available_for_install.get()
                                                    {
                                                        update_installing_for_ui.set(true);
                                                        update_install_progress_for_ui.set(None);
                                                        update_status_for_install.set(
                                                            t!(
                                                                "updater.preparing_download",
                                                                version = update.version
                                                            )
                                                            .into_owned(),
                                                        );
                                                        if !request_update_install(
                                                            update,
                                                            update_install_sender_for_ui.clone(),
                                                            &update_install_in_flight_for_ui,
                                                            updater::RelaunchMode::Visible,
                                                        ) {
                                                            update_installing_for_ui.set(false);
                                                            update_status_for_install.set(
                                                                t!("updater.already_installing")
                                                                    .into_owned(),
                                                            );
                                                            ctx.toast_ok(t!(
                                                                "updater.already_installing"
                                                            ));
                                                        }
                                                    }
                                                },
                                            ),
                                        ),
                                ),
                        )
                        .child(
                            Element::row()
                                .width_match()
                                .spacing(10)
                                .child(
                                    Element::label(
                                        i18n_hub
                                            .tr(|| t!("settings.query_history_hint").into_owned()),
                                    )
                                    .font_size(11.0)
                                    .fg(Color::rgba(235, 241, 255, 175))
                                    .width_match(),
                                )
                                .child(
                                    Element::button(
                                        i18n_hub.tr(|| t!("settings.clear_history").into_owned()),
                                    )
                                    .on_click(move |ctx| {
                                        if let Ok(mut settings) = settings_for_clear_history.write()
                                        {
                                            settings.clear_query_history();
                                            let _ = save_settings(&settings);
                                        }
                                        history_for_clear.borrow_mut().clear();
                                        history_cursor_for_clear.set(None);
                                        ctx.toast_ok(t!("settings.history_cleared"));
                                    }),
                                ),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.native_plugins_hint").into_owned()),
                            )
                            .font_size(12.0)
                            .fg(Color::rgba(235, 241, 255, 160)),
                        )
                        .child(
                            Element::button(i18n_hub.tr(|| t!("settings.apply").into_owned()))
                                .on_click(move |ctx| {
                                    let duration = caret_duration
                                        .get()
                                        .trim()
                                        .parse::<u16>()
                                        .unwrap_or(95)
                                        .clamp(60, 160);
                                    let configuration = HotkeyConfig {
                                        ctrl: activation_ctrl.get(),
                                        alt: activation_alt.get(),
                                        shift: activation_shift.get(),
                                        meta: activation_meta.get(),
                                        key: activation_key.get(),
                                    };
                                    let custom_color =
                                        parse_selection_color(&custom_selection_color.get())
                                            .unwrap_or(0x4c8bf4);
                                    let configured_width = parse_dimension_input(
                                        &launcher_width_input.get(),
                                        MIN_LAUNCHER_WIDTH,
                                        MAX_LAUNCHER_WIDTH,
                                    )
                                    .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                                    let configured_height = parse_dimension_input(
                                        &launcher_height_input.get(),
                                        MIN_LAUNCHER_HEIGHT,
                                        MAX_LAUNCHER_HEIGHT,
                                    )
                                    .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                                    if let Ok(mut settings) = settings_for_apply.write() {
                                        let previous_language = settings.language;
                                        settings.activation_hotkey = configuration;
                                        settings.ignore_hotkeys_in_fullscreen =
                                            ignore_fullscreen.get();
                                        settings.game_mode = game_mode.get();
                                        settings.smooth_caret = smooth_caret.get();
                                        settings.switch_to_english_layout =
                                            switch_to_english_layout.get();
                                        settings.use_system_accent = use_system_accent.get();
                                        settings.custom_selection_color = custom_color;
                                        settings.launcher_width = configured_width;
                                        settings.launcher_height = configured_height;
                                        settings.clear_query_on_activation =
                                            clear_query_on_activation.get();
                                        settings.start_with_windows =
                                            start_with_windows_for_apply.get();
                                        settings.update_checks_enabled =
                                            update_checks_enabled_for_apply.get();
                                        settings.update_interval_hours =
                                            update_interval_hours_for_apply
                                                .get()
                                                .trim()
                                                .parse::<u32>()
                                                .unwrap_or(24)
                                                .clamp(1, 168);
                                        settings.auto_install_updates =
                                            auto_install_updates_for_apply.get();
                                        update_interval_hours_for_apply
                                            .set(settings.update_interval_hours.to_string());
                                        settings.auto_enable_everything =
                                            auto_enable_everything_for_apply.get();
                                        settings.obsidian_enabled =
                                            obsidian_enabled_for_apply.get();
                                        settings.obsidian_alias = obsidian_alias_for_apply.get();
                                        settings.google_enabled = google_enabled_for_apply.get();
                                        settings.google_alias = google_alias_for_apply.get();
                                        settings.monitor_preference =
                                            monitor_preference_from_index(monitor_preference.get());
                                        settings.language = language_preference_from_index(
                                            language_preference_for_apply.get(),
                                        );
                                        settings.smooth_caret_duration_ms = duration;
                                        settings.normalize();
                                        activation_recording_for_apply.set(false);
                                        activation_display_for_apply.set(hotkeys::display_config(
                                            &settings.activation_hotkey,
                                        ));
                                        selection_color
                                            .set(selection_color_for_settings(&settings));
                                        custom_selection_color.set(selection_color_hex(
                                            settings.custom_selection_color,
                                        ));
                                        launcher_width.set(settings.launcher_width);
                                        launcher_height.set(settings.launcher_height);
                                        launcher_width_input
                                            .set(settings.launcher_width.to_string());
                                        launcher_height_input
                                            .set(settings.launcher_height.to_string());
                                        launcher_width_slider.set(dimension_slider_fraction(
                                            settings.launcher_width,
                                            MIN_LAUNCHER_WIDTH,
                                            MAX_LAUNCHER_WIDTH,
                                        ));
                                        launcher_height_slider.set(dimension_slider_fraction(
                                            settings.launcher_height,
                                            MIN_LAUNCHER_HEIGHT,
                                            MAX_LAUNCHER_HEIGHT,
                                        ));
                                        launcher_preview_text.set(
                                            t!(
                                                "settings.visual.client_area",
                                                width = settings.launcher_width,
                                                height = settings.launcher_height
                                            )
                                            .into_owned(),
                                        );
                                        activation_handle_for_apply.set(
                                            hotkeys::activation_hotkey(&settings.activation_hotkey),
                                        );
                                        activation_handle_for_apply.set_enabled(true);
                                        game_mode_status_for_apply
                                            .set(game_mode_label(settings.game_mode));
                                        if settings.auto_enable_everything {
                                            match everything::start_background_if_installed() {
                                                Ok(InstallationState::Installed(_)) => {
                                                    everything_installed.set(true);
                                                    everything_status_for_apply.set(
                                                        t!("everything.detected_enable_ipc")
                                                            .into_owned(),
                                                    );
                                                }
                                                Ok(InstallationState::Missing) => {
                                                    everything_installed.set(false);
                                                    everything_status_for_apply.set(
                                                        t!("everything.not_installed_winget")
                                                            .into_owned(),
                                                    );
                                                }
                                                Err(error) => {
                                                    everything_status_for_apply.set(error)
                                                }
                                            }
                                        } else {
                                            everything_status_for_apply.set(
                                                t!("everything.auto_enable_disabled").into_owned(),
                                            );
                                        }
                                        let _ = save_settings(&settings);
                                        apply_configured_locale(settings.language);
                                        if previous_language != settings.language {
                                            launcher_preview_text.set(
                                                t!(
                                                    "settings.visual.client_area",
                                                    width = launcher_width.get(),
                                                    height = launcher_height.get()
                                                )
                                                .into_owned(),
                                            );
                                            i18n_hub_for_apply.refresh();
                                        }
                                        if let Err(error) =
                                            startup::set_enabled(settings.start_with_windows)
                                        {
                                            ctx.toast_ok(t!(
                                                "settings.startup_failed",
                                                error = error
                                            ));
                                        }
                                        if settings.update_checks_enabled
                                            && update_check_due(&settings)
                                        {
                                            update_status_for_apply
                                                .set(t!("updater.checking_github").into_owned());
                                            request_update_check(
                                                update_sender_for_apply.clone(),
                                                &update_check_in_flight_for_apply,
                                            );
                                        }
                                    }
                                    settings_visible_for_apply.set(false);
                                    let selected_preference =
                                        monitor_preference_from_index(monitor_preference.get());
                                    let applied_width = launcher_width.get() as i32;
                                    let applied_height = launcher_height.get() as i32;
                                    let target_height = if show_results.get() {
                                        applied_height
                                    } else {
                                        COMPACT_WINDOW_HEIGHT
                                    };
                                    request_monitor_position(
                                        &position_for_apply,
                                        selected_preference,
                                        applied_width,
                                        target_height,
                                    );
                                    size_for_apply.set(applied_width, target_height);
                                    ctx.show_window();
                                    ctx.toast_ok(t!("settings.applied"));
                                }),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 3)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(
                            Element::label(i18n_hub.tr(|| t!("settings.everything").into_owned()))
                                .font_size(17.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.everything_tab_desc").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.everything").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.everything_desc").into_owned()),
                                auto_enable_everything,
                            )
                            .on_toggle(move |_| {
                                let enabled = auto_enable_everything_for_toggle.get();
                                if let Ok(mut settings) = settings_for_everything_toggle.write() {
                                    settings.auto_enable_everything = enabled;
                                    settings.normalize();
                                    let _ = save_settings(&settings);
                                }
                                if !enabled {
                                    everything_status_for_toggle
                                        .set(t!("everything.auto_enable_disabled").into_owned());
                                    return;
                                }
                                match everything::start_background_if_installed() {
                                    Ok(InstallationState::Installed(_)) => {
                                        everything_installed_for_toggle.set(true);
                                        everything_status_for_toggle
                                            .set(t!("everything.detected_enable_ipc").into_owned());
                                    }
                                    Ok(InstallationState::Missing) => {
                                        everything_installed_for_toggle.set(false);
                                        everything_status_for_toggle.set(
                                            t!("everything.not_installed_winget").into_owned(),
                                        );
                                    }
                                    Err(error) => everything_status_for_toggle.set(error),
                                }
                            }),
                        ))
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("everything.is_installed").into_owned()),
                            )
                            .font_size(12.0)
                            .fg(Color::rgba(180, 255, 205, 235))
                            .visible_when(move || everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("everything.is_not_installed").into_owned()),
                            )
                            .font_size(12.0)
                            .fg(Color::rgba(255, 225, 175, 235))
                            .visible_when(move || !everything_installed_for_ui.get()),
                        )
                        .child(
                            Element::label_signal(everything_status)
                                .font_size(11.0)
                                .fg(Color::rgba(235, 241, 255, 190))
                                .max_lines(2)
                                .truncate(Truncate::End)
                                .width_match(),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.everything_command").into_owned()),
                            )
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 155))
                            .visible_when(move || !everything_installed_for_ui.get())
                            .width_match(),
                        )
                        .child(
                            Element::button(i18n_hub.tr(|| t!("everything.install").into_owned()))
                                .visible_when(move || !everything_installed_for_ui.get())
                                .on_click(move |ctx| match everything::launch_winget_install() {
                                    Ok(()) => {
                                        everything_status.set(
                                            t!("everything.winget_started_restart").into_owned(),
                                        );
                                        ctx.toast_ok(t!("everything.winget_started_toast"));
                                    }
                                    Err(error) => {
                                        everything_status.set(error.clone());
                                        ctx.toast_ok(error);
                                    }
                                }),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.plugins.title").into_owned()),
                            )
                            .font_size(17.0)
                            .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.plugins.description").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(t!(
                                "settings.plugins.folder",
                                path = native_plugin_install_path()
                            ))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.plugins.config_hint").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.plugins.obsidian").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.plugins.obsidian_desc").into_owned()),
                                obsidian_enabled,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.plugins.action_keyword").into_owned()),
                            Element::text_input(obsidian_alias, "ob").width_match(),
                        ))
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.plugins.obsidian_hint").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 175))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.plugins.google").into_owned()),
                            Element::checkbox(
                                i18n_hub.tr(|| t!("settings.plugins.google_desc").into_owned()),
                                google_enabled,
                            ),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.plugins.action_keyword").into_owned()),
                            Element::text_input(google_alias, "g").width_match(),
                        ))
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.plugins.google_hint").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 175))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 1)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(12)
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.title").into_owned()),
                            )
                            .font_size(17.0)
                            .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.preview_desc").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(3)
                            .truncate(Truncate::End),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.smooth_caret").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::checkbox(
                                        i18n_hub
                                            .tr(|| t!("settings.smooth_caret_desc").into_owned()),
                                        smooth_caret,
                                    )
                                    .width_match(),
                                )
                                .child(Element::text_input(caret_duration, "95").width(76))
                                .child(Element::label("ms").font_size(11.0)),
                        ))
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.selection_color").into_owned()),
                            Element::checkbox(
                                i18n_hub
                                    .tr(|| t!("settings.visual.use_system_accent").into_owned()),
                                use_system_accent,
                            ),
                        ))
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.accent_hint").into_owned()),
                            )
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.visual.preview_hint").into_owned()),
                            )
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 170))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::col()
                                .spacing(8)
                                .visible_when(move || !use_system_accent.get())
                                .child(
                                    Element::text_input(custom_selection_color, "#4C8BF4")
                                        .width_match(),
                                )
                                .child(selection_palette(custom_selection_color)),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.launcher_width").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::slider(launcher_width_slider)
                                        .width(VISUAL_SLIDER_WIDTH),
                                )
                                .child(Element::text_input(launcher_width_input, "420").width(76))
                                .child(
                                    Element::button(
                                        i18n_hub.tr(|| t!("settings.visual.reset").into_owned()),
                                    )
                                    .neutral()
                                    .on_click(move |_| {
                                        let width = DEFAULT_LAUNCHER_WIDTH;
                                        let height = launcher_height.get();
                                        eprintln!(
                                            "Visual width reset clicked: {}x{}",
                                            width, height
                                        );
                                        launcher_width.set(width);
                                        launcher_width_input.set(width.to_string());
                                        launcher_width_slider.set(dimension_slider_fraction(
                                            width,
                                            MIN_LAUNCHER_WIDTH,
                                            MAX_LAUNCHER_WIDTH,
                                        ));
                                        launcher_preview_text.set(
                                            t!(
                                                "settings.visual.client_area",
                                                width = width,
                                                height = height
                                            )
                                            .into_owned(),
                                        );
                                        visual_preview_generation_for_width_reset.set(
                                            visual_preview_generation_for_width_reset
                                                .get()
                                                .saturating_add(1),
                                        );
                                    }),
                                )
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.visual.dip").into_owned()),
                                    )
                                    .font_size(11.0),
                                ),
                        ))
                        .child(
                            Element::label(i18n_hub.tr(|| {
                                t!(
                                    "settings.visual.safe_range",
                                    min = MIN_LAUNCHER_WIDTH,
                                    max = MAX_LAUNCHER_WIDTH
                                )
                                .into_owned()
                            }))
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(Element::field_signal(
                            i18n_hub.tr(|| t!("settings.visual.results_height").into_owned()),
                            Element::row()
                                .width_match()
                                .spacing(8)
                                .child(
                                    Element::slider(launcher_height_slider)
                                        .width(VISUAL_SLIDER_WIDTH),
                                )
                                .child(Element::text_input(launcher_height_input, "382").width(76))
                                .child(
                                    Element::button(
                                        i18n_hub.tr(|| t!("settings.visual.reset").into_owned()),
                                    )
                                    .neutral()
                                    .on_click(move |_| {
                                        let width = launcher_width.get();
                                        let height = DEFAULT_LAUNCHER_HEIGHT;
                                        eprintln!(
                                            "Visual height reset clicked: {}x{}",
                                            width, height
                                        );
                                        launcher_height.set(height);
                                        launcher_height_input.set(height.to_string());
                                        launcher_height_slider.set(dimension_slider_fraction(
                                            height,
                                            MIN_LAUNCHER_HEIGHT,
                                            MAX_LAUNCHER_HEIGHT,
                                        ));
                                        launcher_preview_text.set(
                                            t!(
                                                "settings.visual.client_area",
                                                width = width,
                                                height = height
                                            )
                                            .into_owned(),
                                        );
                                        visual_preview_generation_for_height_reset.set(
                                            visual_preview_generation_for_height_reset
                                                .get()
                                                .saturating_add(1),
                                        );
                                    }),
                                )
                                .child(
                                    Element::label(
                                        i18n_hub.tr(|| t!("settings.visual.dip").into_owned()),
                                    )
                                    .font_size(11.0),
                                ),
                        ))
                        .child(
                            Element::label(i18n_hub.tr(|| {
                                t!(
                                    "settings.visual.safe_range",
                                    min = MIN_LAUNCHER_HEIGHT,
                                    max = MAX_LAUNCHER_HEIGHT
                                )
                                .into_owned()
                            }))
                            .font_size(10.0)
                            .font_size(10.0)
                            .fg(Color::rgba(235, 241, 255, 150)),
                        )
                        .child(
                            Element::label_signal(launcher_preview_text)
                                .font_size(12.0)
                                .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub
                                    .tr(|| t!("settings.visual.native_preview_hint").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 175))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(
                            Element::button(
                                i18n_hub.tr(|| t!("settings.visual.apply").into_owned()),
                            )
                            .on_click(move |ctx| {
                                let mut width = parse_dimension_input(
                                    &launcher_width_input.get(),
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_WIDTH);
                                let mut height = parse_dimension_input(
                                    &launcher_height_input.get(),
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                )
                                .unwrap_or(DEFAULT_LAUNCHER_HEIGHT);
                                let duration = caret_duration
                                    .get()
                                    .trim()
                                    .parse::<u16>()
                                    .unwrap_or(95)
                                    .clamp(60, 160);
                                let Ok(mut settings) = settings_for_visual_apply.write() else {
                                    ctx.toast_ok(t!("settings.lock_failed"));
                                    return;
                                };
                                settings.launcher_width = width;
                                settings.launcher_height = height;
                                settings.smooth_caret = smooth_caret.get();
                                settings.smooth_caret_duration_ms = duration;
                                settings.normalize();
                                width = settings.launcher_width;
                                height = settings.launcher_height;
                                let preference = settings.monitor_preference;
                                if !save_settings(&settings) {
                                    ctx.toast_ok(t!("settings.visual.save_failed"));
                                    return;
                                }
                                launcher_width.set(width);
                                launcher_height.set(height);
                                launcher_width_input.set(width.to_string());
                                launcher_height_input.set(height.to_string());
                                launcher_width_slider.set(dimension_slider_fraction(
                                    width,
                                    MIN_LAUNCHER_WIDTH,
                                    MAX_LAUNCHER_WIDTH,
                                ));
                                launcher_height_slider.set(dimension_slider_fraction(
                                    height,
                                    MIN_LAUNCHER_HEIGHT,
                                    MAX_LAUNCHER_HEIGHT,
                                ));
                                launcher_preview_text.set(
                                    t!(
                                        "settings.visual.client_area",
                                        width = width,
                                        height = height
                                    )
                                    .into_owned(),
                                );
                                eprintln!("Visual Apply dimensions clicked: {}x{}", width, height);
                                settings_visible_for_visual_apply.set(false);
                                let target_height = if show_results_for_visual_apply.get() {
                                    i32::from(height)
                                } else {
                                    COMPACT_WINDOW_HEIGHT
                                };
                                request_monitor_position(
                                    &position_for_visual_apply,
                                    preference,
                                    i32::from(width),
                                    target_height,
                                );
                                size_for_visual_apply.set(i32::from(width), target_height);
                                ctx.show_window();
                                ctx.toast_ok(t!("settings.visual.applied"));
                            }),
                        ),
                ),
        )
        .child(
            Element::scroll()
                .weight(1.0)
                .visible_when(move || settings_tab.get() == 2)
                .child(
                    Element::col()
                        .width_match()
                        .spacing(10)
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.priorities.title").into_owned()),
                            )
                            .font_size(17.0)
                            .fg(Color::WHITE),
                        )
                        .child(
                            Element::label(
                                i18n_hub.tr(|| t!("settings.priorities.description").into_owned()),
                            )
                            .font_size(11.0)
                            .fg(Color::rgba(235, 241, 255, 180))
                            .max_lines(2)
                            .truncate(Truncate::End),
                        )
                        .child(priorities_empty)
                        .child(priority_list),
                ),
        )
