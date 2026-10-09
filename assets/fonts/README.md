# Open Sans

Unmodified OpenSans.ttf and OpenSans-Regular.ttf from Uno.Fonts.OpenSans 2.9.4, the package used by the Linux shell.

Upstream package source: https://github.com/unoplatform/uno.fonts at a5a82943e36c97ce02cea3b0fba105f309216948.
Font source: https://github.com/googlefonts/opensans.
Copyright 2020 The Open Sans Project Authors. SIL Open Font License 1.1; see ../../licenses/open-sans-OFL.txt.

The variable font includes weight and width axes. WinUI uses FontWeight to select regular and semibold for display text. Native TextBox and PasswordBox use the normal-width static regular face because their edit renderer condenses the variable face. Linux retains Uno's original font URI and static-font manifest so Skia uses the existing weight-specific files for both display and input text.
