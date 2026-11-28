# Build the independent C host for version 2 .emb images.
#
# Implementation files arrive one agent branch at a time, so sources are
# discovered rather than assumed. `make` reports what is missing instead of
# failing with "No rule to make target" while branches are integrated.

CC ?= cc
CFLAGS ?= -std=c11 -Wall -Wextra -O2
CPPFLAGS ?= -I.
LDFLAGS ?=
LDLIBS ?= -lm

TARGET := emrun

# The complete set of implementations needed for a working interpreter.
SOURCES := main.c value.c emb.c vm.c vm_call.c vm_method.c builtins.c host.c
PRESENT := $(filter $(SOURCES),$(wildcard *.c))
MISSING := $(filter-out $(PRESENT),$(SOURCES))
OBJECTS := $(PRESENT:.c=.o)
HEADERS := $(wildcard *.h)

.DEFAULT_GOAL := all
.PHONY: all check-sources test clean

all:
	@if [ -n "$(MISSING)" ]; then \
		echo "bootstrap-c: skipping $(TARGET) (missing: $(MISSING))" >&2; \
		echo "Merge the remaining round4 implementation branches, then rerun make." >&2; \
	else \
		$(MAKE) --no-print-directory $(TARGET); \
	fi

$(TARGET): $(OBJECTS)
	$(CC) $(LDFLAGS) -o $@ $(OBJECTS) $(LDLIBS)

ifneq ($(strip $(OBJECTS)),)
$(OBJECTS): $(HEADERS)
endif

%.o: %.c
	$(CC) $(CFLAGS) $(CPPFLAGS) -c -o $@ $<

check-sources:
	@printf 'present: %s\n' '$(PRESENT)'
	@if [ -n '$(MISSING)' ]; then \
		printf 'missing: %s\n' '$(MISSING)'; \
	else \
		printf 'all sources present\n'; \
	fi

test: all
	@./tests/test_cli.sh

clean:
	rm -f $(OBJECTS) $(TARGET)
