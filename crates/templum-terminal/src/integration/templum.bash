# Templum's shell integration for bash, started with `--init-file` this file. It tells Templum
# when a prompt starts (OSC 133 A), when a command starts and ends (C and D, with the exit code)
# and the current folder (OSC 7), and changes nothing else. Bash 4.4 or newer reports commands;
# any bash, prompts and the folder.
# The startup files part follows VS Code's shellIntegration-bash.sh (MIT, (c) Microsoft).

# --init-file took the place of the startup files: read the ones bash would have read.
if [[ -n "${TEMPLUM_SHELL_LOGIN-}" ]]; then
    builtin unset TEMPLUM_SHELL_LOGIN
    [[ -r /etc/profile ]] && builtin . /etc/profile
    for __templum_file in ~/.bash_profile ~/.bash_login ~/.profile; do
        if [[ -r "$__templum_file" ]]; then
            builtin . "$__templum_file"
            break
        fi
    done
    builtin unset __templum_file
else
    [[ -r /etc/bash.bashrc ]] && builtin . /etc/bash.bashrc
    [[ -r ~/.bashrc ]] && builtin . ~/.bashrc
fi

if [[ -z "${__templum_installed-}" ]]; then
    __templum_installed=1

    # First in PROMPT_COMMAND, for the command's status; it gives it back to what runs next.
    # The prompt is printed after PROMPT_COMMAND: A marks its start.
    __templum_precmd() {
        builtin local status=$?
        builtin printf '\e]133;D;%s\a\e]7;file://%s%s\a\e]133;A\a' "$status" "${HOSTNAME-}" "${PWD//\%/%25}"
        __templum_ps0
        builtin return "$status"
    }
    # PS0 marks a command's start; a prompt theme may set it again later.
    __templum_ps0() {
        [[ "${PS0-}" == *'\e]133;C\a'* ]] || PS0="${PS0-}"'\e]133;C\a'
    }
    __templum_ps0
    if [[ -n "${BLE_VERSION-}" ]]; then
        # ble.sh, a line editor, puts PROMPT_COMMAND back as it was when it attaches, and draws
        # its first prompt before its hooks run: its own hook, and a first report now.
        blehook PRECMD+=__templum_precmd
        __templum_precmd
    else
        PROMPT_COMMAND="__templum_precmd${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
    fi
fi
