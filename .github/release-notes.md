
## Opening it the first time

KickCut is not signed with an Apple Developer certificate, so macOS refuses it
on the first double-click and says it cannot be opened. Right-click the app in
Applications, choose **Open**, then **Open** again in the dialog. That records
your decision once; every later launch is normal.

The same thing in one command, if you prefer:

```
xattr -dr com.apple.quarantine /Applications/KickCut.app
```

A certificate is an annual cost, and paying it would remove a warning rather
than change what the app does — so the app stays unsigned and the hash below is
published instead.

## Verifying your download

To confirm the file is the one published here and was not altered on its way to
you, compare its fingerprint:

```
shasum -a 256 @NAME@
```

```
@SHA@
```

That proves the file matches what was built from this repository. It does not
make an unknown program safe — it answers "is this the real one", which is the
question worth asking when an installer reaches you through chat rather than
from the release page.
