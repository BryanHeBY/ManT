int
mant_mandoc_read_bundle(struct mparse *parser, const char *requested_path)
{
	const struct mant_mandoc_source	*source;
	struct mant_mandoc_resolved_source resolved;
	const char			*current, *slash;
	unsigned char			*resolved_data;
	char				*resolved_path;
	char				*path;
	char				*beside;
	size_t				 prefix_length;
	int				 error_number, status;

	if (bundle_sources == NULL && source_resolver == NULL)
		return 0;
	path = normalize_requested_bundle_path(requested_path);
	if (path == NULL)
		return -1;
	if (bundle_sources == NULL) {
		memset(&resolved, 0, sizeof(resolved));
		current = mandoc_msg_getinfilename();
		status = source_resolver(source_resolver_context, path, current,
		    &resolved);
		if (status <= 0) {
			switch (status) {
			case -2:
				error_number = EPERM;
				break;
			case -3:
				error_number = EIO;
				break;
			default:
				error_number = ENOENT;
				break;
			}
			free(path);
			errno = error_number;
			return -1;
		}
		if (resolved.path == NULL || resolved.data == NULL ||
		    !is_safe_bundle_path(resolved.path)) {
			free(path);
			errno = EINVAL;
			return -1;
		}
		resolved_path = copy_string(resolved.path);
		resolved_data = malloc(resolved.length == 0 ? 1 :
		    resolved.length);
		if (resolved_path == NULL || resolved_data == NULL) {
			free(resolved_path);
			free(resolved_data);
			free(path);
			errno = ENOMEM;
			return -1;
		}
		if (resolved.length != 0)
			memcpy(resolved_data, resolved.data, resolved.length);
		mparse_readmem(parser, resolved_data, resolved.length,
		    resolved_path);
		free(resolved_data);
		free(resolved_path);
		free(path);
		return 1;
	}
	source = find_bundle_source(path);
	if (source == NULL &&
	    (current = mandoc_msg_getinfilename()) != NULL &&
	    (slash = strrchr(current, '/')) != NULL) {
		prefix_length = (size_t)(slash - current + 1);
		beside = malloc(prefix_length + strlen(path) + 1);
		if (beside == NULL) {
			free(path);
			errno = ENOMEM;
			return -1;
		}
		memcpy(beside, current, prefix_length);
		strcpy(beside + prefix_length, path);
		source = find_bundle_source(beside);
		free(beside);
	}
	if (source == NULL) {
		free(path);
		errno = ENOENT;
		return -1;
	}
	mparse_readmem(parser, source->data, source->length, source->path);
	free(path);
	return 1;
}

static const struct mant_mandoc_source *
find_bundle_source(const char *path)
{
	size_t i;

	if (path == NULL)
		return NULL;
	for (i = 0; i < bundle_source_count; i++)
		if (bundle_sources[i].path != NULL &&
		    strcmp(bundle_sources[i].path, path) == 0)
			return bundle_sources + i;
	return NULL;
}

static int
is_safe_bundle_path(const char *path)
{
	const char *component, *end;

	if (path == NULL || *path == '\0' || *path == '/' ||
	    strchr(path, '\\') != NULL)
		return 0;
	component = path;
	while (*component != '\0') {
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end == component ||
		    (end - component == 1 && component[0] == '.') ||
		    (end - component == 2 && component[0] == '.' &&
		    component[1] == '.'))
			return 0;
		if (*end == '/' && end[1] == '\0')
			return 0;
		component = *end == '/' ? end + 1 : end;
	}
	return 1;
}

/* Normalize harmless current-directory components in an untrusted request. */
static char *
normalize_requested_bundle_path(const char *path)
{
	const char	*component, *end;
	char		*normalized, *output;
	size_t		 length;

	if (path == NULL || *path == '\0' || *path == '/' ||
	    strchr(path, '\\') != NULL) {
		errno = EPERM;
		return NULL;
	}
	length = strlen(path);
	normalized = malloc(length + 1);
	if (normalized == NULL) {
		errno = ENOMEM;
		return NULL;
	}
	output = normalized;
	component = path;
	while (*component != '\0') {
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end == component ||
		    (end - component == 2 && component[0] == '.' &&
		    component[1] == '.') ||
		    (*end == '/' && end[1] == '\0'))
			goto unsafe;
		if (!(end - component == 1 && component[0] == '.')) {
			if (output != normalized)
				*output++ = '/';
			memcpy(output, component, (size_t)(end - component));
			output += end - component;
		}
		component = *end == '/' ? end + 1 : end;
	}
	if (output == normalized)
		goto unsafe;
	*output = '\0';
	return normalized;

unsafe:
	free(normalized);
	errno = EPERM;
	return NULL;
}

#ifndef MANDOC_MEMORY_ONLY
/* Open `path` under `base`, leaving errno describing any failure. */
static int
open_under_base(const char *base, const char *path, int flags, mode_t mode)
{
	char	*resolved;
	int	 fd, saved_errno;

	if (base == NULL) {
		errno = ENOENT;
		return -1;
	}
	resolved = malloc(strlen(base) + strlen(path) + 2);
	if (resolved == NULL) {
		errno = ENOMEM;
		return -1;
	}
	sprintf(resolved, "%s/%s", base, path);
	fd = openat(AT_FDCWD, resolved, flags, mode);
	saved_errno = errno;
	free(resolved);
	errno = saved_errno;
	return fd;
}

/*
 * Open a relative path without following any link below an explicit root.
 * The root itself is caller-approved and may be a link; every component after
 * it is opened relative to the previous directory descriptor with O_NOFOLLOW.
 */
static int
open_beneath_root(const char *path, int flags, mode_t mode)
{
	char	*copy, *component, *separator;
	int	 dirfd, fd, saved_errno;

	if (source_root == NULL || path == NULL || *path == '\0' ||
	    *path == '/') {
		errno = EPERM;
		return -1;
	}
	dirfd = openat(AT_FDCWD, source_root,
	    O_RDONLY | O_DIRECTORY | O_CLOEXEC);
	if (dirfd == -1)
		return -1;
	copy = copy_string(path);
	if (copy == NULL) {
		close(dirfd);
		errno = ENOMEM;
		return -1;
	}
	component = copy;
	fd = -1;
	for (;;) {
		while (*component == '/')
			component++;
		if (*component == '\0') {
			errno = EINVAL;
			break;
		}
		separator = strchr(component, '/');
		if (separator != NULL)
			*separator = '\0';
		if (strcmp(component, "..") == 0) {
			errno = EPERM;
			break;
		}
		if (strcmp(component, ".") == 0) {
			if (separator == NULL) {
				errno = EINVAL;
				break;
			}
			component = separator + 1;
			continue;
		}
		if (separator == NULL) {
			fd = openat(dirfd, component,
			    flags | O_NOFOLLOW | O_CLOEXEC, mode);
			break;
		}
		fd = openat(dirfd, component,
		    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
		if (fd == -1)
			break;
		close(dirfd);
		dirfd = fd;
		fd = -1;
		component = separator + 1;
	}
	saved_errno = errno;
	free(copy);
	close(dirfd);
	errno = saved_errno;
	return fd;
}

/* Resolve a bare include beside the source, still walking from the root. */
static int
open_beside_source(const char *path, int flags, mode_t mode)
{
	const char	*relative;
	char		*combined;
	size_t		 root_length;
	int		 fd, saved_errno;

	if (source_root == NULL || source_dir == NULL) {
		errno = ENOENT;
		return -1;
	}
	root_length = strlen(source_root);
	if (strncmp(source_dir, source_root, root_length) != 0 ||
	    (root_length != 1 && source_dir[root_length] != '/' &&
	    source_dir[root_length] != '\0')) {
		errno = EPERM;
		return -1;
	}
	relative = source_dir + root_length;
	while (*relative == '/')
		relative++;
	if (*relative == '\0')
		return open_beneath_root(path, flags, mode);
	combined = malloc(strlen(relative) + strlen(path) + 2);
	if (combined == NULL) {
		errno = ENOMEM;
		return -1;
	}
	sprintf(combined, "%s/%s", relative, path);
	fd = open_beneath_root(combined, flags, mode);
	saved_errno = errno;
	free(combined);
	errno = saved_errno;
	return fd;
}

/* Resolve includes against the original source tree without changing cwd. */
int
mant_mandoc_source_open(const char *path, int flags, ...)
{
	int		 fd, saved_errno, top_level_path;
	mode_t		 mode;
	size_t		 source_path_length;
	va_list		 arguments;

	mode = 0;
	if (flags & O_CREAT) {
		va_start(arguments, flags);
		mode = (mode_t)va_arg(arguments, int);
		va_end(arguments);
	}
	/*
	 * The top-level file is caller-selected and remains readable under
	 * IncludePolicy::Deny. Every later open comes from roff `.so` handling
	 * and must be authorized explicitly instead of treating an unset root as
	 * permission to fall back to the process working directory.
	 */
	top_level_path = 0;
	if (source_path_pending && source_path != NULL) {
		source_path_length = strlen(source_path);
		top_level_path = strcmp(path, source_path) == 0 ||
		    (strncmp(path, source_path, source_path_length) == 0 &&
		    strcmp(path + source_path_length, ".gz") == 0);
	}
	if (top_level_path) {
		fd = openat(AT_FDCWD, path, flags, mode);
		if (fd != -1)
			source_path_pending = 0;
		return fd;
	}
	if (source_includes_allowed == 0) {
		errno = EPERM;
		return -1;
	}
	if (*path == '/') {
		if (source_root_strict &&
		    (source_path == NULL || strcmp(path, source_path) != 0)) {
			errno = EPERM;
			return -1;
		}
		return openat(AT_FDCWD, path, flags, mode);
	}
	if (source_root_strict && source_root == NULL) {
		errno = ENOMEM;
		return -1;
	}
	if (source_root == NULL && source_dir == NULL)
		return openat(AT_FDCWD, path, flags, mode);
	if (source_root_strict && !is_safe_relative_path(path)) {
		errno = EPERM;
		return -1;
	}

	/*
	 * Try the stripped hierarchy root first (`.so man1/foo.1`), then the
	 * unstripped stub directory (`.so last.1` next to the stub). man(1)
	 * accepts both spellings, and mparse_open retries each base with a
	 * `.gz` suffix, so compressed targets resolve without extra handling.
	 */
	fd = source_root_strict ? open_beneath_root(path, flags, mode) :
	    open_under_base(source_root, path, flags, mode);
	if (fd != -1)
		return fd;
	saved_errno = errno;
	if (source_dir != NULL &&
	    (source_root == NULL || strcmp(source_dir, source_root) != 0)) {
		fd = source_root_strict ? open_beside_source(path, flags, mode) :
		    open_under_base(source_dir, path, flags, mode);
		if (fd != -1)
			return fd;
		saved_errno = errno;
	}
	if (source_root_strict) {
		errno = saved_errno;
		return -1;
	}
	fd = openat(AT_FDCWD, path, flags, mode);
	if (fd == -1)
		errno = saved_errno;
	return fd;
}

/* Reject lexical escapes when a caller supplied an explicit include root. */
static int
is_safe_relative_path(const char *path)
{
	const char *component, *end;

	component = path;
	while (*component != '\0') {
		while (*component == '/')
			component++;
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end - component == 2 && component[0] == '.' &&
		    component[1] == '.')
			return 0;
		component = end;
	}
	return 1;
}
#endif

