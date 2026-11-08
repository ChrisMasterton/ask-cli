#!/bin/bash

# OP-01: shared install/upgrade/removal/restoration lifecycle. Make enters after
# its release prerequisite; the standalone installer enters at the build state.
set -e

ask_install_lifecycle() {
    local operation="${1-}" presentation="${2-}"
    local install_target=/usr/local/bin/ask
    local backup_target=/usr/local/bin/ask.backup
    local release_binary=target/release/ask
    local lifecycle_state failure_status=0 echo_commands=true
    local ignore_failures=false failure_continuation make_error_prefix=make
    local make_flags="${MAKEFLAGS-}"
    local short_make_flags="${make_flags%% *}"
    local make_level="${MAKELEVEL-0}"

    # Make normally echoes top-level sudo recipes, but hides backup/restore
    # recipes. Retain that distinction, including make -s/--silent.
    if [[ "$short_make_flags" != -* && "$short_make_flags" == *s* ]]; then
        echo_commands=false
    fi
    if [[ "$presentation" == make && "$short_make_flags" != -* && "$short_make_flags" == *i* ]]; then
        ignore_failures=true
    fi
    if [[ "$presentation" == make && "$make_level" =~ ^[0-9]+$ ]] && (( 10#$make_level > 1 )); then
        make_error_prefix="make[$((10#$make_level - 1))]"
    fi

    case "$operation:$presentation" in
        install:script) lifecycle_state=building ;;
        install:make) lifecycle_state=announcing_install ;;
        uninstall:make) lifecycle_state=announcing_removal ;;
        *) printf 'Usage: install-lifecycle.sh install make|script, or uninstall make\n' >&2; return 2 ;;
    esac

    while :; do
        case "$lifecycle_state" in
            building)
                failure_continuation=announcing_install
                printf '🔨 Building release version...\n'
                if cargo build --release; then
                    lifecycle_state=announcing_install
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            announcing_install)
                if [[ "$presentation" == make ]]; then
                    printf 'Installing to /usr/local/bin (requires sudo)...\n'
                else
                    printf '📦 Installing to /usr/local/bin...\n'
                    printf 'This will require sudo permissions.\n'
                fi
                lifecycle_state=inspecting_existing
                ;;
            inspecting_existing)
                if [[ -f "$install_target" ]]; then
                    lifecycle_state=backing_up
                else
                    lifecycle_state=copying
                fi
                ;;
            backing_up)
                failure_continuation=copying
                if [[ "$presentation" == make ]]; then
                    printf 'Backing up existing binary to /usr/local/bin/ask.backup\n'
                else
                    printf "⚠️  Existing 'ask' found at /usr/local/bin/ask\n"
                    printf 'Creating backup at /usr/local/bin/ask.backup\n'
                fi
                if sudo cp "$install_target" "$backup_target"; then
                    lifecycle_state=copying
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            copying)
                failure_continuation=setting_permissions
                if [[ "$presentation" == make && "$echo_commands" == true ]]; then
                    printf 'sudo cp target/release/ask /usr/local/bin/ask\n'
                fi
                if sudo cp "$release_binary" "$install_target"; then
                    lifecycle_state=setting_permissions
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            setting_permissions)
                failure_continuation=installed
                if [[ "$presentation" == make && "$echo_commands" == true ]]; then
                    printf 'sudo chmod +x /usr/local/bin/ask\n'
                fi
                if sudo chmod +x "$install_target"; then
                    lifecycle_state=installed
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            installed)
                if [[ "$presentation" == make ]]; then
                    printf '✅ Installed successfully!\n'
                    printf "Run 'ask --help' to see the new options\n"
                else
                    printf '✅ Installation complete!\n\n'
                    printf "You can now use 'ask' from anywhere in your terminal.\n"
                    printf 'Try: ask --help\n\n'
                    printf 'Confirmation: Enter/y runs, n cancels, s skips a command.\n'
                fi
                return 0
                ;;
            announcing_removal)
                printf 'Removing ask from /usr/local/bin (requires sudo)...\n'
                lifecycle_state=removing
                ;;
            removing)
                failure_continuation=inspecting_backup
                if [[ "$echo_commands" == true ]]; then
                    printf 'sudo rm -f /usr/local/bin/ask\n'
                fi
                if sudo rm -f "$install_target"; then
                    lifecycle_state=inspecting_backup
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            inspecting_backup)
                if [[ -f "$backup_target" ]]; then
                    lifecycle_state=restoring
                else
                    lifecycle_state=uninstalled
                fi
                ;;
            restoring)
                failure_continuation=uninstalled
                printf 'Restoring backup...\n'
                if sudo mv "$backup_target" "$install_target"; then
                    lifecycle_state=uninstalled
                else
                    failure_status=$?
                    lifecycle_state=failed
                fi
                ;;
            uninstalled)
                printf '✅ Uninstalled successfully!\n'
                return 0
                ;;
            failed)
                # Preserve partial effects: no implicit rollback or cleanup.
                if [[ "$ignore_failures" == true ]]; then
                    # Previously these were separate Make recipes. With -i,
                    # Make reported each error and ran the following recipe.
                    if [[ "$echo_commands" == true ]]; then
                        printf '%s: [%s] Error %s (ignored)\n' "$make_error_prefix" "$operation" "$failure_status" >&2
                    fi
                    lifecycle_state="$failure_continuation"
                else
                    return "$failure_status"
                fi
                ;;
        esac
    done
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    ask_install_lifecycle "$@"
fi
