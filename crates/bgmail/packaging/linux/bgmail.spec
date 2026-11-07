# RPM of a prebuilt BGMail (Fedora, RHEL and clones, openSUSE). Built by
# script/bundle-linux-rpm from a script/bundle-linux tree:
#   rpmbuild -bb --define "bgmail_version 1.0.0~beta.1" --define "bundle /path/bgmail.app"

Name:           bgmail
Version:        %{bgmail_version}
Release:        1
Summary:        Native desktop e-mail client built on GPUI
License:        MIT
URL:            https://github.com/belizariogr/bgmail
ExclusiveArch:  x86_64 aarch64

# Linked libraries (xcb, xkbcommon, libc) are found by rpm's automatic
# dependencies. GPUI dlopen()s the rest, so they are named here by soname,
# which every RPM distro provides whatever its package names.
Requires:       libwayland-client.so.0()(64bit)
Requires:       libwayland-cursor.so.0()(64bit)
Requires:       libvulkan.so.1()(64bit)
Requires:       libfontconfig.so.1()(64bit)
Requires:       xdg-utils

# Prebuilt and already stripped: no debuginfo package, no brp rewriting.
%global debug_package %{nil}
%global __os_install_post %{nil}

%description
BGMail is a native desktop e-mail client built on GPUI, the GPU-accelerated UI
framework from Zed. IMAP/SMTP and Gmail accounts, a macOS Mail-style layout,
light and dark themes, and a native, sanitized HTML reader.

%install
mkdir -p %{buildroot}%{_prefix}
cp -r %{bundle}/. %{buildroot}%{_prefix}/

%files
%{_bindir}/bgmail
%{_datadir}/applications/BGMail.desktop
%{_datadir}/icons/hicolor/*/apps/bgmail.png
